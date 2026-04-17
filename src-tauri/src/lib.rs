use std::time::Duration;

use anyhow::{anyhow, Result};
use chromiumoxide::{Browser, BrowserConfig, Handler};
use futures::StreamExt;
use reqwest::Client;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager};
use tauri_plugin_log::log;
use tauri_plugin_shell::{process::CommandEvent, ShellExt};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    agents::orchestrator::{self, system_prompt},
    models::models::{download_model, get_models, seed_models},
    state::AppState,
    storage::database::initialize_storage,
};

mod agents;
mod intelligence;
mod models;
mod state;
mod storage;

#[tauri::command]
async fn new_conversation(app_handle: AppHandle) {
    let state = app_handle.state::<AppState>();
    let mut conversation = state.conversation.lock().await;
    let mut conversation_id = state.conversation_id.lock().await;

    let conn = state.connection.lock().await;

    *conversation_id = Uuid::new_v4().to_string();
    conversation.clear();
    conversation.push(json!({
        "role": "system",
        "content": system_prompt(),
    }));

    // Insert new chat into chats
    let _ = conn
        .execute(
            "INSERT INTO chats (id, messages, ui_messages) VALUES (?1, ?2, ?3)",
            (&*conversation_id, "", ""),
        )
        .unwrap();

    // TODO: Emit new conversation created, so frontend can load a blank slate
}

#[tauri::command]
async fn save_conversation(ui_messages: String, app_handle: AppHandle) {
    let state = app_handle.state::<AppState>();
    let conversation = state.conversation.lock().await;
    let conversation_id = state.conversation_id.lock().await;
    let conn = state.connection.lock().await;

    let messages = serde_json::to_string(&*conversation).unwrap();
    let _ = conn
        .execute(
            "
                UPDATE chats SET messages = ?1, ui_messages = ?2 WHERE id = ?3
            ",
            (messages, ui_messages, &*conversation_id),
        )
        .unwrap();
}

#[tauri::command]
async fn load_conversation(conversation_id: String, app_handle: AppHandle) -> String {
    let state = app_handle.state::<AppState>();
    let mut conversation = state.conversation.lock().await;
    let mut conv_id = state.conversation_id.lock().await;
    let conn = state.connection.lock().await;

    let (messages, ui_messages): (String, String) = conn
        .query_row(
            "SELECT messages, ui_messages FROM chats WHERE id = ?1",
            [&conversation_id],
            |row| Ok((row.get(0).unwrap(), row.get(1).unwrap())),
        )
        .unwrap();

    *conv_id = conversation_id;
    *conversation = serde_json::from_str(&messages).unwrap();
    ui_messages
}

#[derive(Serialize)]
struct ChatEntry {
    id: String,
    title: String,
}

#[tauri::command]
async fn get_chats(app_handle: AppHandle) -> Vec<ChatEntry> {
    let state = app_handle.state::<AppState>();
    let conn = state.connection.lock().await;
    let mut stmt = conn
        .prepare("SELECT id, title FROM chats WHERE messages != ''")
        .unwrap();
    stmt.query_map([], |row| {
        Ok(ChatEntry {
            id: row.get(0)?,
            title: row.get(1)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[tauri::command]
async fn send_message(query: String, app_handle: AppHandle) {
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        match state.llama_child.lock().await.as_ref() {
            Some(_) => {}
            None => {
                println!("llama-server not up yet!");
                return;
            }
        }

        {
            let mut conversation = state.conversation.lock().await;
            if conversation.len() == 1 {
                let title: String = query.chars().take(40).collect();
                let conv_id = state.conversation_id.lock().await;
                let conn = state.connection.lock().await;
                conn.execute(
                    "UPDATE chats SET title = ?1 WHERE id = ?2",
                    (&title, &*conv_id),
                )
                .unwrap();
            }
            conversation.push(json!({
                "role": "user",
                "content": query
            }));
        }
        let _ = orchestrator::run(&app_handle).await.unwrap();
    });
}

#[tauri::command]
async fn launch_model(model_id: String, app_handle: AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let file_name = {
        let conn = state.connection.lock().await;
        conn.query_row(
            "SELECT file_name FROM models WHERE id = ?1 AND downloaded = 1",
            [&model_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| format!("Model '{}' not found or not downloaded", model_id))?
    };

    let model_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models")
        .join(&file_name);
    if !model_path.exists() {
        return Err(format!("Model file not found at: {}", model_path.display()));
    }

    // Kill existing server if one is running
    {
        let mut child_lock = state.llama_child.lock().await;
        if let Some(old) = child_lock.take() {
            old.kill().ok();
        }
    }

    // Wait for port to actually close
    loop {
        match tokio::net::TcpStream::connect("127.0.0.1:8080").await {
            Err(_) => break, // port is dead
            Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }

    let (mut rx, child) = app_handle
        .shell()
        .sidecar("llama-server")
        .map_err(|e| e.to_string())?
        .args(["-m", model_path.to_str().unwrap(), "-c", "8192"])
        .spawn()
        .map_err(|e| e.to_string())?;

    loop {
        match rx.recv().await {
            Some(CommandEvent::Stdout(line)) => {
                let text = String::from_utf8_lossy(&line);
                println!("[llama-server] {}", text);
                if text.contains("all slots are idle") {
                    *state.llama_child.lock().await = Some(child);
                    return Ok(());
                }
            }
            Some(CommandEvent::Stderr(line)) => {
                let text = String::from_utf8_lossy(&line);
                println!("[llama-server] {}", text);
                if text.contains("all slots are idle") {
                    *state.llama_child.lock().await = Some(child);
                    return Ok(());
                }
            }
            Some(CommandEvent::Error(e)) => return Err(e),
            None => return Err("llama-server exited unexpectedly".to_string()),
            _ => {}
        }
    }
}

pub async fn ensure_browser(app_handle: &AppHandle) -> Result<()> {
    let state = app_handle.state::<AppState>();
    if state.browser.lock().await.is_none() {
        start_browser(app_handle).await?
    }
    Ok(())
}

async fn try_launch_browser(browser_data_dir: &std::path::Path) -> Result<(Browser, Handler)> {
    Browser::launch(
        BrowserConfig::builder()
            // .with_head()
            .user_data_dir(browser_data_dir)
            .arg("--start-maximized")
            .arg("--disable-blink-features=AutomationControlled")
            .viewport(None)
            .build()
            .map_err(|_| anyhow!("Error: Can't build browser!"))?,
    )
    .await
    .map_err(|e| anyhow!(e))
}

pub async fn start_browser(app_handle: &AppHandle) -> Result<()> {
    log::info!("Trying to launch browser!");
    let app_data_dir = app_handle.path().app_data_dir().unwrap();
    let browser_data_dir = app_data_dir.join("browser_profile");

    std::fs::create_dir_all(&browser_data_dir)?;

    let is_first_run = !browser_data_dir.join("Default").exists();

    let (browser, mut handler) = {
        let result = try_launch_browser(&browser_data_dir).await;
        match result {
            Ok(b) => b,
            Err(e) if is_first_run => {
                log::warn!("First run launch failed (expected): {}. Retrying...", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
                try_launch_browser(&browser_data_dir).await?
            }
            Err(e) => return Err(e),
        }
    };

    log::info!("Browser launched successfuly!");

    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                println!("[Browser Handler] error: {}", e);
                let state = app_handle_clone.state::<AppState>();
                state.browser.lock().await.take();
                break;
            }
        }
    });
    log::info!("Browser state set!");

    let state = app_handle.state::<AppState>();
    *state.browser.lock().await = Some(browser);

    let browser_guard = state.browser.lock().await;
    if let Some(b) = browser_guard.as_ref() {
        let mut attempts = 0;
        let page = loop {
            let pages = b.pages().await?;
            if let Some(p) = pages.into_iter().next() {
                break p;
            }
            attempts += 1;
            if attempts >= 5 {
                return Err(anyhow!("Browser never created a tab"));
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        };
        page.goto("about:blank").await?;
    }
    log::info!("about:blank opened!");

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("logs".to_string()),
                    },
                ))
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let app_data_dir = handle.path().app_data_dir().unwrap();
            std::fs::create_dir_all(&app_data_dir).unwrap();

            let system_prompt = system_prompt();

            let db_path = app_data_dir.join("recursio.sqlite3");
            let conn = Connection::open(&db_path).unwrap();

            // Creates chats table if not exists
            let _ = initialize_storage(&conn).unwrap();
            let _ = seed_models(&conn).unwrap();
            // Initialize New Chat
            let chat_id = Uuid::new_v4().to_string();
            let _ = conn.execute("INSERT INTO chats (id) VALUES (?1)", (&chat_id,));
            handle.manage(AppState {
                browser: Mutex::new(None),
                http_client: Client::new(),
                conversation: Mutex::new(vec![json!(
                    {
                        "role": "system",
                        "content": system_prompt
                    }
                )]),
                conversation_id: Mutex::new(chat_id),
                connection: Mutex::new(conn),
                llama_child: Mutex::new(None),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = app.try_state::<AppState>() {
                        let mut browser_guard = state.browser.lock().await;
                        let browser = browser_guard.as_mut();
                        if let Some(b) = browser {
                            let _ = b.close().await.unwrap();
                        }
                        let conn = state.connection.lock().await;
                        conn.execute("DELETE FROM chats WHERE messages = ''", [])
                            .unwrap();
                        let mut llama_child = state.llama_child.lock().await;
                        if let Some(child) = llama_child.take() {
                            child.kill().unwrap();
                        }
                    }
                });
            };
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            send_message,
            new_conversation,
            save_conversation,
            load_conversation,
            get_chats,
            get_models,
            download_model,
            launch_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
