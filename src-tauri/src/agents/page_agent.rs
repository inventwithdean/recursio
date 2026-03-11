use anyhow::{anyhow, Result};
use chromiumoxide::Page;
use reqwest::Client;
use serde_json::{json, Value};

use crate::intelligence::{query_llm, AgentResponse};

// What the PageAgent will return after completing its task
#[derive(Debug)]
pub struct PageAgentResult {
    pub content: String,
    pub final_url: String,
}

// The state the PageAgent sees each loop iteration
struct PageState {
    url: String,
    title: String,
    text: String,
}

async fn get_page_state(page: &Page) -> Result<PageState> {
    let url: String = page
        .evaluate("window.location.href")
        .await?
        .into_value()
        .unwrap_or_default();

    let title: String = page
        .evaluate("document.title")
        .await?
        .into_value()
        .unwrap_or_default();

    let raw_text: String = page
        .evaluate("document.body.innerText")
        .await?
        .into_value()
        .unwrap_or_default();

    let text = raw_text
        .char_indices()
        .nth(6000)
        .map(|(i, _)| &raw_text[..i])
        .unwrap_or(&raw_text)
        .to_string();

    Ok(PageState { url, title, text })
}

fn build_state_message(state: &PageState) -> String {
    format!(
        "URL: {}\nTitle: {}\n\n--- PAGE TEXT ---\n{}\n\n",
        state.url, state.title, state.text,
    )
}

fn system_prompt(query: &str) -> String {
    format!(
        r#"You are a focused page extraction agent. Your ONLY job is to answer this query:

"{}"

Call done() with the full extracted content.
"#,
        query
    )
}

fn tools() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "done",
                "description": "Call this when you have fully answered the query. Provide the extracted content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "result": {
                            "type": "string",
                            "description": "The final extracted content that answers the query."
                        }
                    },
                    "required": ["result"]
                }
            }
        },
    ])
}

pub async fn run(client: &Client, page: &Page, query: &str) -> Result<PageAgentResult> {
    let tools = tools();
    let system_prompt = system_prompt(query);
    let mut messages: Vec<Value> = vec![json!({"role": "system", "content": system_prompt})];

    // Limit to max_steps
    let state = get_page_state(page).await?;
    let state_msg = build_state_message(&state);

    println!("[PageAgent] url={}", state.url);

    messages.push(json!({"role": "user", "content": state_msg}));

    let response = query_llm(client, &json!(messages), &tools, None).await?;

    match response {
        AgentResponse::Text(text) => {
            // LLM gave a text answer directly, treat it as the result.
            println!("[PageAgent] Got text answer directly: {}", text);
            let final_url: String = page
                .evaluate("window.location.href")
                .await?
                .into_value()
                .unwrap_or_default();
            return Ok(PageAgentResult {
                content: text,
                final_url,
            });
        }
        AgentResponse::ToolCall {
            name, arguments, ..
        } => {
            let args: Value = serde_json::from_str(&arguments).unwrap_or(json!({}));

            match name.as_str() {
                "done" => {
                    let result = args["result"].as_str().unwrap_or("").to_string();
                    println!("[PageAgent] Done. Extracted {} chars.", result.len());
                    let final_url: String = page
                        .evaluate("window.location.href")
                        .await?
                        .into_value()
                        .unwrap_or_default();
                    Ok(PageAgentResult {
                        content: result,
                        final_url,
                    })
                }

                unknown => {
                    // Handle unknown tool, and continue LLM loop
                    println!("[PageAgent] Unknown tool: {}", unknown);
                    Err(anyhow!("Page Agent called unknown tool"))
                }
            }
        }

        AgentResponse::Error(e) => Err(anyhow!(format!("PageAgent LLM Error: {}", e))),
    }
}
