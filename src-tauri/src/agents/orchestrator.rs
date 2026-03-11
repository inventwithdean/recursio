use anyhow::{anyhow, Result};
use std::time::Duration;
use tauri_plugin_log::log;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    agents::{page_agent, search_agent},
    intelligence::{query_llm, AgentResponse},
    state::AppState,
};

fn tools() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "research",
                "description": "Search the web for information and extract it from the best result page. Use for any factual question, 'what happens in X', 'who is Y', 'how does Z work', etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "search_query": {
                            "type": "string",
                            "description": "The search query to find relevant pages, e.g. 'Breaking Bad season 2 episode 3 plot summary'"
                        },
                        "page_extraction_goal": {
                            "type": "string",
                            "description": "What to extract from the page once found, e.g. 'Extract the full episode plot summary'"
                        }
                    },
                    "required": ["search_query", "page_extraction_goal"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Open a specific URL and extract information from it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The full URL to open."
                        },
                        "extraction_goal": {
                            "type": "string",
                            "description": "What to extract or do on the page."
                        }
                    },
                    "required": ["url", "extraction_goal"]
                }
            }
        }
    ])
}

pub fn system_prompt() -> String {
    let now = chrono::Local::now();
    let date = now.format("%B %d, %Y").to_string();
    println!("Date: {}", date);
    format!(r#"You are a helpful AI assistant with access to a web browser.

When the user asks about something that requires looking up information (plot summaries, news, facts, prices, etc.), use the research tool.
When the user gives you a specific URL to visit, use open_url.
For anything you already know well and doesn't need current data, answer directly.
Today is: {}
If you encounter a captcha, tell the user and ask them to solve it in the browser window."#, date).to_string()
}

pub async fn run(app_handle: &AppHandle) -> Result<()> {
    let tools = tools();
    let state = app_handle.state::<AppState>();
    let client = &state.http_client;

    // Orchestrator Loop
    loop {
        let messages = state.conversation.lock().await.clone();
        let response = query_llm(client, &json!(messages), &tools, Some(app_handle)).await?;
        match response {
            AgentResponse::Text(text) => {
                // println!("\nAssistant: {}\n", text);
                state
                    .conversation
                    .lock()
                    .await
                    .push(json!({"role": "assistant", "content": text}));
                // EMIT Assistant Message
                let _ = app_handle.emit("assistant_message", json!(text))?;

                return Ok(()); // Break the loop
            }
            AgentResponse::ToolCall {
                id,
                name,
                arguments,
            } => {
                println!("[Orchestrator] Tool call: {} args={}", name, arguments);
                state.conversation.lock().await.push(json!({
                    "role": "assistant",
                        "tool_calls": [{
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": arguments }
                        }]
                }));

                let args: serde_json::Value = serde_json::from_str(&arguments).unwrap_or(json!({}));

                let tool_result: String = match name.as_str() {
                    "research" => {
                        // Handle Research Tool
                        let search_query = args["search_query"].as_str().unwrap_or("").to_string();
                        let page_goal = args["page_extraction_goal"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        // TODO: Emit Research Start
                        println!("search_agent::run");
                        match search_agent::run(client, &search_query, &page_goal, 3, app_handle)
                            .await
                        {
                            Ok(result) => {
                                let source_urls = result.source_urls.join("\n");
                                format!("Sources: {}\n\n{}", source_urls, result.content)
                            }
                            Err(e) => {
                                log::info!("[Orchestrator] Error in search_agent::run : {}", e);
                                format!("Research failed: {}. You may want to try a different search query.", e)
                            }
                        }
                    }
                    "open_url" => {
                        // Handle Open URL Tool
                        let url = args["url"].as_str().unwrap_or("").to_string();
                        let goal = args["extraction_goal"]
                            .as_str()
                            .unwrap_or("Extract the main content.")
                            .to_string();

                        if url.is_empty() {
                            "Error: No URL provided.".to_string()
                        } else {
                            let guard = state.browser.lock().await;
                            let browser = guard.as_ref();
                            match browser.as_ref() {
                                Some(b) => match b.new_page(&url).await {
                                    Ok(page) => {
                                        let _ = page.wait_for_navigation().await;
                                        tokio::time::sleep(Duration::from_millis(800)).await;

                                        match page_agent::run(client, &page, &goal).await {
                                            Ok(result) => {
                                                let _ = page.close().await;
                                                format!(
                                                    "Source: {}\n\n{}",
                                                    result.final_url, result.content
                                                )
                                            }
                                            Err(e) => {
                                                let _ = page.close().await;
                                                format!("Failed to extract from {}: {}", url, e)
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        format!("Failed to open {}: {}", url, e)
                                    }
                                },
                                None => {
                                    format!("Browser isn't available right now!")
                                }
                            }
                        }
                    }
                    unknown => {
                        // Handle unknown tool
                        format!("Unknown tool '{}'. Use research or open_url.", unknown)
                    }
                };

                println!("[Tool result] {} chars", tool_result.len());

                state.conversation.lock().await.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": tool_result
                }));

                //  Continue Inner Loop
            }

            AgentResponse::Error(e) => {
                println!("\nError: {}\n", e);
                return Err(anyhow!(format!("Error: {}", e)));
            }
        }
    }
}
