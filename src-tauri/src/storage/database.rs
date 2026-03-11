use anyhow::Result;
use rusqlite::Connection;

pub fn initialize_storage(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS chats (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL DEFAULT 'untitled',
            messages TEXT NOT NULL DEFAULT '',
            ui_messages TEXT NOT NULL DEFAULT ''
        );
        ",
    )?;
    let _ = conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS models (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            description TEXT NOT NULL,
            file_name TEXT NOT NULL,
            url TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            vram_gb INTEGER NOT NULL,
            downloaded INTEGER NOT NULL DEFAULT 0
        );
        ",
    );
    Ok(())
}
