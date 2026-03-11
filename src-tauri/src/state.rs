use chromiumoxide::Browser;
use reqwest::Client;
use rusqlite::Connection;
use serde_json::Value;
use tauri_plugin_shell::process::CommandChild;
use tokio::sync::Mutex;

pub struct AppState {
    pub browser: Mutex<Option<Browser>>,
    pub http_client: Client,
    pub conversation: Mutex<Vec<Value>>,
    pub conversation_id: Mutex<String>,
    pub connection: Mutex<Connection>,
    pub llama_child: Mutex<Option<CommandChild>>,
}
