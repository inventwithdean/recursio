use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
struct SearchResult {
    title: Option<String>,
    snippet: Option<String>,
    date: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SerperSearchResult {
    organic: Vec<SearchResult>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResult {
    title: Option<String>,
    image_url: Option<String>,
    image_width: Option<usize>,
    image_height: Option<usize>,
    pub thumbnail_url: Option<String>,
    thumbnail_width: Option<usize>,
    thumbnail_height: Option<usize>,
    source: Option<String>,
    domain: Option<String>,
    pub link: Option<String>,
    position: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct SerperImagesResult {
    pub images: Vec<ImageResult>,
}

enum SerperEndpoint {
    SEARCH,
    IMAGES,
}

async fn request_serper(
    data: &Value,
    endpoint: SerperEndpoint,
    client: &reqwest::Client,
) -> Result<String, anyhow::Error> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("X-API-key", std::env::var("SERPER_API_KEY")?.parse()?);
    headers.insert("Content-Type", "application/json".parse()?);
    let endpoint = match endpoint {
        SerperEndpoint::SEARCH => "search",
        SerperEndpoint::IMAGES => "images",
    };
    let url = format!("https://google.serper.dev/{}", endpoint);
    let response = client
        .request(reqwest::Method::POST, url)
        .headers(headers)
        .json(&data)
        .send()
        .await?;

    let body = response.text().await?;
    Ok(body)
}

pub async fn search_web(
    query: &str,
    client: &reqwest::Client,
) -> Result<SerperSearchResult, anyhow::Error> {
    println!("Serper Web Search: {}", query);
    let data = json!({
        "q": query
    });
    let content = request_serper(&data, SerperEndpoint::SEARCH, client).await?;
    // println!("{}", body);
    let content_json: SerperSearchResult = serde_json::from_str(&content)?;
    // println!(
    //     "Search results:\n{}",
    //     serde_json::to_string_pretty(&content_json)?
    // );
    Ok(content_json)
}

pub async fn search_images(
    query: &str,
    client: &reqwest::Client,
) -> Result<SerperImagesResult, anyhow::Error> {
    println!("Serper Image Search: {}", query);
    let data = json!({
        "q": query
    });
    let content = request_serper(&data, SerperEndpoint::IMAGES, client).await?;
    let content_json: SerperImagesResult = serde_json::from_str(&content)?;
    // println!(
    //     "Images Result:\n{}",
    //     serde_json::to_string_pretty(&content_json)?
    // );
    Ok(content_json)
}
