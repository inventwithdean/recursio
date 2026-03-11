use std::path::Path;

use futures::StreamExt;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

#[derive(Serialize)]
pub struct Model {
    id: String,
    display_name: String,
    description: String,
    file_name: String,
    url: String,
    size_bytes: i64,
    vram_gb: i32,
    downloaded: bool,
}

pub fn seed_models(conn: &Connection) -> anyhow::Result<()> {
    let models = [
        (
            "recursio",
            "Recursio",
            "Fast and capable. Great for everyday tasks.",
            "Qwen3.5-4B-Q4_K_M.gguf",
            "https://models.recursio.app/Qwen3.5-4B-Q4_K_M.gguf",
            "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
            2740937888i64,
            6i32,
        ),
        (
            "recursio_pro",
            "Recursio Pro",
            "Deeper reasoning and stronger analysis. For complex tasks.",
            "Qwen3.5-9B-Q4_K_M.gguf",
            "https://models.recursio.app/Qwen3.5-9B-Q4_K_M.gguf",
            "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
            5680522464i64,
            12i32,
        ),
        (
            "recursio_ultra",
            "Recursio Ultra",
            "Maximum intelligence. For research and hard problems.",
            "Qwen3.5-35B-A3B-Q4_K_M.gguf",
            "https://models.recursio.app/Qwen3.5-35B-A3B-Q4_K_M.gguf",
            "3b46d1066bc91cc2d613e3bc22ce691dd77e6f0d33c9060690d24ce6de494375",
            22016023168i64,
            24i32,
        ),
    ];

    for (id, display_name, description, file_name, url, sha256, size_bytes, vram_gb) in &models {
        conn.execute(
            "
            INSERT OR IGNORE INTO models
                (id, display_name, description, file_name, url, sha256, size_bytes, vram_gb, downloaded)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
            (id, display_name, description, file_name, url, sha256, size_bytes, vram_gb))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_models(app_handle: AppHandle) -> Vec<Model> {
    let state = app_handle.state::<AppState>();
    let conn = state.connection.lock().await;
    let mut stmt = conn
        .prepare("SELECT id, display_name, description, file_name, url, size_bytes, vram_gb, downloaded FROM models")
        .unwrap();
    stmt.query_map([], |row| {
        Ok(Model {
            id: row.get(0)?,
            display_name: row.get(1)?,
            description: row.get(2)?,
            file_name: row.get(3)?,
            url: row.get(4)?,
            size_bytes: row.get(5)?,
            vram_gb: row.get(6)?,
            downloaded: row.get::<_, i32>(7)? == 1,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[tauri::command]
pub async fn download_model(model_id: String, app_handle: AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let (url, file_name, expected_hash) = {
        let conn = state.connection.lock().await;
        conn.query_row(
            "SELECT url, file_name, sha256 FROM models WHERE id = ?1",
            [&model_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
    };
    let models_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");

    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
    let tmp_path = models_dir.join(format!("{}.tmp", file_name));
    let final_path = models_dir.join(&file_name);

    let client = reqwest::Client::new();

    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if content_type.contains("text/html") {
        return Err("Server returned HTML instead of a binary file - check the URL".to_string());
    }

    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = response.bytes_stream();
    let mut last_emit = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if downloaded - last_emit >= (total / 100).max(10_000_000) {
            last_emit = downloaded;
            let _ = app_handle.emit(
                "download_progress",
                json!({"model_id": model_id, "bytes_downloaded": downloaded, "total_bytes": total}),
            );
        }
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| e.to_string())?;
    drop(file);

    // Hash check
    let actual_hash = hash_file(&tmp_path)?;
    if actual_hash != expected_hash {
        std::fs::remove_file(&tmp_path).ok();
        let _ = app_handle.emit(
            "download_error",
            json!({
                "model_id": model_id,
                "error": "Hash mismatch - file may be corrupted, please try again."
            }),
        );
        return Err("Hash mismatch".to_string());
    }

    std::fs::rename(&tmp_path, &final_path).map_err(|e| e.to_string())?;
    let conn = state.connection.lock().await;
    conn.execute(
        "UPDATE models SET downloaded = 1 WHERE id = ?1",
        [&model_id],
    )
    .map_err(|e| e.to_string())?;
    let _ = app_handle.emit("download_complete", json!({"model_id": model_id}));
    Ok(())
}
