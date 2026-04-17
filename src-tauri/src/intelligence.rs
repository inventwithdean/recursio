use anyhow::Result;
use reqwest::{Client, Response};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub enum AgentResponse {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    Error(String),
}
pub async fn query_llm(
    client: &Client,
    messages: &Value,
    tools: &Value,
    app_handle: Option<&AppHandle>,
) -> Result<AgentResponse> {
    let should_stream = app_handle.is_some();

    let payload = json!({
        "model": "recursio",
        "messages": messages,
        "tools": tools,
        "stream": should_stream
    });

    // println!("{}", serde_json::to_string_pretty(&payload)?);

    let response = client
        .post("http://localhost:8080/v1/chat/completions")
        .json(&payload)
        .send()
        .await?;

    if !should_stream {
        return parse_blocking(response).await;
    }

    parse_streaming(response, app_handle.unwrap()).await
}

async fn parse_blocking(response: Response) -> Result<AgentResponse> {
    let res_json: Value = response.json().await?;
    let message_data = &res_json["choices"][0]["message"];

    if let Some(tool_calls) = message_data.get("tool_calls") {
        if let Some(first_tool) = tool_calls.get(0) {
            let id = first_tool["id"].as_str().unwrap_or("").to_string();
            let name = first_tool["function"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let arguments = first_tool["function"]["arguments"]
                .as_str()
                .unwrap_or("")
                .to_string();

            return Ok(AgentResponse::ToolCall {
                id,
                name,
                arguments,
            });
        }
    }

    if let Some(content) = message_data.get("content").and_then(|c| c.as_str()) {
        return Ok(AgentResponse::Text(content.to_string()));
    }

    Ok(AgentResponse::Error(
        "Failed to parse LLM responses properly".to_string(),
    ))
}

async fn parse_streaming(mut response: Response, app_handle: &AppHandle) -> Result<AgentResponse> {
    // let mut stream = response.bytes_stream();

    let mut full_content = String::new();
    let mut tool_id = String::new();
    let mut tool_name = String::new();
    let mut tool_args = String::new();
    let mut is_tool_call = false;

    while let Some(chunk) = response.chunk().await? {
        let text = match std::str::from_utf8(&chunk) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for line in text.lines() {
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line["data: ".len()..];
            if data == "[DONE]" {
                break;
            }
            let Ok(json) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let delta = &json["choices"][0]["delta"];

            // Tool call - aggregate
            if let Some(tcs) = delta.get("tool_calls") {
                is_tool_call = true;
                let tc = &tcs[0];
                if let Some(id) = tc["id"].as_str() {
                    tool_id = id.to_string()
                };
                if let Some(name) = tc["function"]["name"].as_str() {
                    tool_name = name.to_string();
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    tool_args.push_str(args);
                }
            }

            if let Some(content) = delta["content"].as_str() {
                if !content.is_empty() {
                    full_content.push_str(content);
                    let _ = app_handle.emit("assistant_chunk", content);
                }
            }
        }
    }

    if is_tool_call {
        return Ok(AgentResponse::ToolCall {
            id: tool_id,
            name: tool_name,
            arguments: tool_args,
        });
    }

    Ok(AgentResponse::Text(full_content))
}
