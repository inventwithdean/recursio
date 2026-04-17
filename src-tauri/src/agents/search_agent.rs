// Handle one specific query and reason to search it
// Gets:
//    query: general query like "spiderman 2026 movies",
//    page_query, that tells it what to extract from the pages it gets.
// Delegates each page to page_agent by giving it the page_query

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{
    agents::page_agent::{self, PageAgentResult},
    ensure_browser,
    intelligence::{query_llm, AgentResponse},
    state::AppState,
};

// What SearchAgent will return on successful completion
pub struct ResearchAgentResult {
    pub content: String,
    pub source_urls: Vec<String>,
}

// A candidate URL with metadta for the Ranker
#[derive(Debug, Serialize, Clone)]
struct Candidate {
    id: String, // UUID string
    url: String,
    title: String,
    snippet: String,
}

async fn fetch_candidates(
    app_handle: &AppHandle,
    query: &str,
    max: usize,
) -> Result<Vec<Candidate>> {
    let encoded = urlencoding::encode(query);
    let search_url = format!("https://html.duckduckgo.com/html/?q={}", encoded);
    let state = app_handle.state::<AppState>();
    let guard = state.browser.lock().await;

    let browser = match guard.as_ref() {
        Some(b) => b,
        None => return Err(anyhow!("Browser not available")),
    };

    let page = browser.new_page("about:blank").await?;
    page.wait_for_navigation().await?;
    page.enable_stealth_mode().await?;
    page.goto(&search_url).await?;
    page.wait_for_navigation().await?;
    tokio::time::sleep(Duration::from_millis(800)).await;

    let raw: Value = page
        .evaluate(
            r#"
        (() => {
            return Array.from(document.querySelectorAll('.result'))
                .slice(0, 10)
                .map(r => {
                    const a = r.querySelector('a.result__a');
                    const snip = r.querySelector('.result__snippet');
                    const raw = a ? a.getAttribute('href') : '';
                    // Unwrap DDG's redirect: /l/?uddg=<encoded-real-url>&...
                    let url = '';
                    if (raw) {
                        try {
                            const params = new URL('https://duckduckgo.com' + raw).searchParams;
                            url = decodeURIComponent(params.get('uddg') || params.get('u') || raw);
                        } catch {
                            url = raw;
                        }
                    }
                    return {
                        url,
                        title: a ? a.innerText.trim() : '',
                        snippet: snip ? snip.innerText.trim() : ''
                    };
                })
                .filter(r => r.url && r.url.startsWith('http') && !r.url.includes('duckduckgo.com'));
        })()
    "#,
        )
        .await?
        .into_value()?;

    let candidates: Vec<Candidate> = raw
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .take(max)
        .map(|v| Candidate {
            id: Uuid::new_v4().to_string(),
            url: v["url"].as_str().unwrap_or("").to_string(),
            title: v["title"].as_str().unwrap_or("").to_string(),
            snippet: v["snippet"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    page.close().await?;

    Ok(candidates)
}

// Helper function to rank the search results based on the query
async fn rank_candidates(
    client: &Client,
    query: &str,
    candidates: &[Candidate],
) -> Result<Vec<usize>> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let candidate_list: Vec<String> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| format!("[{}] {}\n    {}\n    {}", i, c.title, c.url, c.snippet))
        .collect();

    let prompt = format!(
        "You are a search result ranker. Given the query and a list of search results, \
        pick the best URLs to answer the query. Rank from most to least relevant.\n\n\
        Query: \"{}\"Results: \n{}",
        query,
        candidate_list.join("\n\n")
    );
    let tools = json!([{
        "type": "function",
        "function": {
            "name": "rank",
            "description": "Return the indices of the best results, ordered from best to worst.",
            "parameters": {
                "type": "object",
                "properties": {
                    "indices": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Ordered list of result indices, best first. E.g. [2, 0, 4]"
                    }
                },
                "required": ["indices"]
            }
        }
    }]);

    let messages = json!([
        { "role": "system", "content": "You are a search result ranker. Always call the rank tool." },
        { "role": "user", "content": prompt }
    ]);

    let response = query_llm(client, &messages, &tools, None).await?;

    match response {
        AgentResponse::ToolCall { arguments, .. } => {
            let args: Value = serde_json::from_str(&arguments).unwrap_or(json!({}));
            let indices: Vec<usize> = args["indices"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .filter(|&i| i < candidates.len())
                .collect();
            Ok(indices)
        }
        _ => Ok((0..candidates.len()).collect()),
    }
}

fn distiller_system_prompt() -> String {
    r#"You are a distiller. You receive extractions from multiple web pages about the same query. Your
job is to synthesize them into one authoritative, well-structured answer.
e.g., if the query is asking for some details, then use all the sources to craft an actual response.
Do NOT invent or infer facts not present in the sources.
    "#.to_string()
}

async fn distill(client: &Client, query: &str, extractions: &[PageAgentResult]) -> Result<String> {
    if extractions.is_empty() {
        return Err(anyhow!("No extractions to distill"));
    }

    if extractions.len() == 1 {
        return Ok(extractions[0].content.clone());
    }

    let sources_block: String = extractions
        .iter()
        .enumerate()
        .map(|(i, e)| format!("--- SOURCE {} ({}) --- \n{}", i + 1, e.final_url, e.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let system_prompt = distiller_system_prompt();
    let user_prompt = format!("Query: \"{}\"\n\n{}", query, sources_block);
    let messages = json!([
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_prompt}
    ]);

    let response = query_llm(client, &messages, &json!([]), None).await?;
    match response {
        AgentResponse::Text(t) => Ok(t),
        AgentResponse::ToolCall { .. } => Err(anyhow!("Distiller unexpectedly called a tool.")),
        AgentResponse::Error(e) => Err(anyhow!(format!("Distiller LLM error: {}", e))),
    }
}

pub async fn run(
    client: &Client,
    query: &str,
    page_query: &str,
    max_candidates_to_try: usize,
    app_handle: &AppHandle,
) -> Result<ResearchAgentResult> {
    match ensure_browser(app_handle).await {
        Ok(_) => {}
        Err(e) => return Err(anyhow!("Error opening the browser: {}. Tell the user about this error. And ask them to close the browser window if open. Don't try anymore tools until its fixed.", e)),
    }
    let search_id = Uuid::new_v4().to_string();
    println!("[SearchAgent] Searching for: {}", query);

    // Emit to frontend the query
    let _ = app_handle.emit(
        "search_query",
        json!({"query": query, "search_id": search_id}),
    )?;
    let candidates;

    match fetch_candidates(app_handle, query, 8).await {
        Ok(c) => candidates = c,
        Err(e) => {
            println!("Error in fetch_candidates: {}", e);
            return Err(anyhow!(
                "Browser isn't open. Stop using Tools. Tell the user."
            ));
        }
    }
    println!("[SearchAgent] Got {} candidates", candidates.len());

    // Emit to frontend
    let payload = json!({"search_results": candidates, "query": query, "search_id": search_id});
    let _ = app_handle.emit("search_results", payload);

    if candidates.is_empty() {
        return Err(anyhow!("No Search Results found. The User may be offline. Tell the user, and don't use tools anymore."));
    }

    for (i, c) in candidates.iter().enumerate() {
        println!("[{}] {} - {}", i, c.title, c.url);
    }

    let ranked = rank_candidates(client, query, &candidates).await?;
    println!("[SearchAgent] Ranked order: {:?}", ranked);

    let to_try: Vec<usize> = if ranked.is_empty() {
        (0..candidates.len().min(max_candidates_to_try)).collect()
    } else {
        ranked.into_iter().take(max_candidates_to_try).collect()
    };

    let mut page_agent_results: Vec<PageAgentResult> = Vec::new();

    for idx in to_try {
        let candidate = &candidates[idx];
        println!("[SearchAgent] Trying [{}]: {}", idx, candidate.url);

        let _ = app_handle.emit(
            "search_result_status_change",
            json!({"search_id": search_id, "result_id": candidate.id, "status": "active"}),
        )?;
        let state = app_handle.state::<AppState>();
        let guard = state.browser.lock().await;

        let browser = match guard.as_ref() {
            Some(b) => b,
            None => return Err(anyhow!("Browser not available")),
        };

        let page = match browser.new_page(&candidate.url).await {
            Ok(p) => {
                p.wait_for_navigation().await?;
                p.enable_stealth_mode().await?;
                p
            }
            Err(e) => {
                println!("[Search Agent] Failed to open page: {}", e);
                let _ = app_handle.emit(
                    "search_result_status_change",
                    json!({"search_id": search_id, "result_id": candidate.id, "status": "failed"}),
                )?;
                continue;
            }
        };

        drop(guard); // Drop the guard
        tokio::time::sleep(Duration::from_millis(800)).await;

        // Run Page Agent on current candidate and page_query

        match page_agent::run(client, &page, page_query).await {
            Ok(result) => {
                println!("[SearchAgent] PageAgent succeeded on {}", candidate.url);
                let _ = app_handle.emit(
                    "search_result_status_change",
                    json!({"search_id": search_id, "result_id": candidate.id, "status": "visited"}),
                )?;
                let _ = page.close().await;
                page_agent_results.push(PageAgentResult {
                    content: result.content,
                    final_url: result.final_url,
                });
                // Continue to other sources
            }

            Err(e) => {
                println!("[SearchAgent] PageAgent failed on {}: {}", candidate.url, e);
                let _ = app_handle.emit(
                    "search_result_status_change",
                    json!({"search_id": search_id, "result_id": candidate.id, "status": "failed"}),
                )?;
                let _ = page.close().await;
            }
        }
    }

    if page_agent_results.is_empty() {
        return Err(anyhow!("All candidates failed to yield an answer."));
    }

    let source_urls: Vec<String> = page_agent_results
        .iter()
        .map(|e| e.final_url.clone())
        .collect();
    println!(
        "[ResearchAgent] Distilling {} extractions",
        page_agent_results.len()
    );

    match distill(client, query, &page_agent_results).await {
        Ok(content) => Ok(ResearchAgentResult {
            content,
            source_urls: source_urls,
        }),
        Err(e) => Err(anyhow!(format!("Could not distill the responses: {}", e))),
    }
}
