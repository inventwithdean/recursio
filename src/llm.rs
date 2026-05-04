use std::env;

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::serper;

#[derive(Serialize, Deserialize)]
pub struct LLMNode {
    pub title: String,
    pub description: String,
    pub image_query: Option<String>,
}

// Using Option<> is a good way to handle every type of response,
// so if LLM returns nothing for nodes field, it will be None
#[derive(Serialize, Deserialize)]
struct SimpleQueryResponse {
    nodes: Option<Vec<LLMNode>>,
    search_query: Option<String>,
}

pub async fn get_llm_response(
    messages: &Value,
    client: &reqwest::Client,
) -> Result<String, anyhow::Error> {
    let base_url = "https://api.deepseek.com/v1/chat/completions";
    let api_key = env::var("DEEPSEEK_API_KEY")?;
    let mut headers = HeaderMap::new();

    headers.insert("Authorization", format!("Bearer {api_key}").parse()?);
    headers.insert("Accept", "application/json".parse()?);
    headers.insert("Content-Type", "application/json".parse()?);

    let payload = json!(
        {
            "model": "deepseek-v4-flash",
            "messages": messages,
            "response_format": {
                "type": "json_object",
                // "json_schema": get_story_response_format()
            },
            "max_tokens": 1000,
            "thinking": {"type": "disabled"},
        }
    );
    let res = client
        .post(base_url)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    if !res.status().is_success() {
        let err = res.text().await?;
        anyhow::bail!("Something went wrong: {err}")
    }
    let res_json = res.json::<Value>().await?;
    let content_str = res_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}");
    Ok(content_str.to_string())
}

pub async fn simple_query(
    query: &str,
    client: &reqwest::Client,
) -> Result<Vec<LLMNode>, anyhow::Error> {
    let mut messages = json!([
        {
            "role": "system",
            "content": get_system_prompt_simple_query()
        },
        {
            "role": "user",
            "content": query
        }
    ]);

    let mut content_str = get_llm_response(&messages, client).await?;
    // println!("Raw content: {}", content_str);
    let mut parsed_content: SimpleQueryResponse = serde_json::from_str(&content_str)?;
    println!("{}", serde_json::to_string_pretty(&parsed_content)?);

    // Keep searching untill we get nodes object
    while let Some(search_query) = parsed_content.search_query {
        messages
            .as_array_mut()
            .unwrap()
            .push(json!({"role": "assistant", "content": content_str}));
        let results = serper::search_web(&search_query, client).await?;
        let results = serde_json::to_string_pretty(&results)?;
        messages
            .as_array_mut()
            .unwrap()
            .push(json!({"role": "user", "content": results}));
        content_str = get_llm_response(&messages, client).await?;
        parsed_content = serde_json::from_str(&content_str)?;
        println!("{}", serde_json::to_string_pretty(&parsed_content)?);
    }

    if let Some(nodes) = parsed_content.nodes {
        Ok(nodes)
    } else {
        Ok(vec![])
    }
}

fn get_system_prompt_simple_query() -> String {
    return r#"You are a simple web assistant for a graph based app, you will receive simple user queries and have to return structured json response!
e.g. if user asks for xai's owner, notice how you don't have to include xai in the title, because the graph probably already contains xAI as parent node.
you should output:
{
    "nodes": [
        {
            "title": "Elon Musk",
            "description": "2-3 lines of description."
        }
    ]
}

if user asks for xai,
you should output:
{
    "nodes": [
        {
            "title": "xAI",
            "description": "2-3 lines of description about xAI."
        }
    ]
}

So, if user asks about 'DeepMind Demis Hassabis primary fields', there should be n number of nodes with their title being the field names, like "AGI" etc, and description accordingly.
e.g. if user asks spacex primary focus, the title should be "Space Rockets" or whatever you like.
You can also use multiple objects inside nodes, each having title and description if needed.

And you should know that if there are multiple keywords like 'Artificial Intelligence main players Microsoft primary focus',
then they're most probably asking about Microsoft's primary focus which is windows, azure etc. not just related to AI, as its a graph, and you don't have to create nodes for Artificial Intelligence or Microsoft as they probably already exist in the graph.

If user asks OSI model -- (layers) -> Transport Layer -- (protocols) ->
Then you only need to make nodes of the different protocols, do not make nodes of transport layer, as they already exist in the graph

If relation asks a question, the title should be the answer, like (is multimodal?), yes/no.

Today is 04 May, 2026."#
    .to_string();
}


fn get_system_prompt_simple_query_with_web() -> String {
    return r#"You are a simple web assistant for a graph based app, you will receive simple user queries and have to return structured json response!
e.g. if user asks for xai's owner, notice how you don't have to include xai in the title, because the graph probably already contains xAI as parent node.
you should output:
{
    "nodes": [
        {
            "title": "Elon Musk",
            "description": "2-3 lines of description."
        }
    ]
}

if user asks for xai,
you should output:
{
    "nodes": [
        {
            "title": "xAI",
            "description": "2-3 lines of description about xAI."
        }
    ]
}

So, if user asks about 'DeepMind Demis Hassabis primary fields', there should be n number of nodes with their title being the field names, like "AGI" etc, and description accordingly.
e.g. if user asks spacex primary focus, the title should be "Space Rockets" or whatever you like.
You can also use multiple objects inside nodes, each having title and description if needed.

Sometimes, you may need to search the web before you send the nodes object, for queries that you think are better to search the web before sending the nodes.
For that, you simply output search object like this 

{
    "search_query": "your search query"
}

Then the system will search the web and attach the web results, then you can return actual nodes.
e.g. if user asks something which you have no idea about, something/someone very specific, you should probably use the search first.
Remember to make the title useful, e.g. if user asks about gemma 4 31B dense MMLU score, the title should be the percentage, not "MMLU Score", same applies to everything.

If you want to include the image so that user can see the image too, like in case of people, then you can include image_query in the node as well, 
which the system will use to search the web for images and give one of those images to users.

Like if user asks about supernatural dean winchester actor,
then you could return
{
    "nodes": [
        {
            "title": "Jensen Ackles",
            "description": "2-3 lines of description about him",
            "image_query": "Jensen Ackles"
        }
    ]
}

This is very useful in case of specific people, places, things etc. Remember to use a descriptive image_query. If you don't have a full name, consider web search.
When you are not sure about a person, don't assume, use the search_query. And you should know that if there are multiple keywords like 'Artificial Intelligence main players Microsoft primary focus',
then they're most probably asking about Microsoft's primary focus which is windows, azure etc. not just related to AI, as its a graph, and you don't have to create nodes for Artificial Intelligence or Microsoft as they probably already exist in the graph.

If user asks OSI model -- (layers) -> Transport Layer -- (protocols) ->
Then you only need to make nodes of the different protocols, do not make nodes of transport layer, as they already exist in the graph

If relation asks a question, the title should be the answer, like (is multimodal?), yes/no.

Remember to not use stale knowledge, and use web search when you think its better to search!
Today is 04 May, 2026."#
    .to_string();
}
