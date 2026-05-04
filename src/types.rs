use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Image {
    pub url: String,
    pub link: String,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub title: String,
    pub description: String,
    pub image: Option<Image>,
}
