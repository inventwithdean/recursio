use serde::{Deserialize, Serialize};
use sqlx::{
    prelude::FromRow,
    types::chrono::{DateTime, Utc},
};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct GraphListItem {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct GraphItem {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub data: serde_json::Value,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

pub async fn does_graph_exists(graph_id: &str, pool: &sqlx::PgPool) -> Result<bool, anyhow::Error> {
    let graph_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM graphs WHERE id = $1)")
            .bind(graph_id)
            .fetch_one(pool)
            .await?;
    Ok(graph_exists)
}

pub async fn get_graph_owner(graph_id: &str, pool: &sqlx::PgPool) -> Result<String, anyhow::Error> {
    let user_id: String = sqlx::query_scalar("SELECT user_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_one(pool)
        .await?;
    Ok(user_id)
}

/// Inserts an Untitled graph
pub async fn insert_new_graph(
    graph_id: &str,
    user_id: &str,
    data: &serde_json::Value,
    pool: &sqlx::PgPool,
) -> Result<bool, anyhow::Error> {
    let result = sqlx::query(
        r#"
    INSERT INTO graphs (id, user_id, data)
    VALUES ($1, $2, $3)
    ON CONFLICT (id) DO NOTHING
    "#,
    )
    .bind(graph_id)
    .bind(user_id)
    .bind(data)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Returns true if updated, false if not owner
pub async fn update_graph_if_owner(
    graph_id: &str,
    user_id: &str,
    data: &serde_json::Value,
    pool: &sqlx::PgPool,
) -> Result<bool, anyhow::Error> {
    let result = sqlx::query(
        r#"
    UPDATE graphs
    SET data = $1
    WHERE id = $2 AND user_id = $3
    "#,
    )
    .bind(data)
    .bind(graph_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn get_graph_data(
    graph_id: &str,
    user_id: &str,
    pool: &sqlx::PgPool,
) -> Result<GraphItem, anyhow::Error> {
    let graph_item: GraphItem =
        sqlx::query_as("SELECT * FROM graphs WHERE id = $1 AND user_id = $2")
            .bind(graph_id)
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(graph_item)
}

/// Toggles graph visibility
pub async fn toggle_graph_visibility(
    graph_id: &str,
    user_id: &str,
    is_public: bool,
    pool: &sqlx::PgPool,
) -> Result<bool, anyhow::Error> {
    let result = sqlx::query(r#"UPDATE graphs SET is_public = $1 WHERE id = $2 AND user_id = $3"#)
        .bind(is_public)
        .bind(graph_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Returns the graph if the graph is set to public
pub async fn get_graph_public(
    graph_id: &str,
    pool: &sqlx::PgPool,
) -> Result<GraphItem, anyhow::Error> {
    let graph_item: GraphItem =
        sqlx::query_as("SELECT * FROM graphs WHERE id = $1 AND is_public = true;")
            .bind(graph_id)
            .fetch_one(pool)
            .await?;
    Ok(graph_item)
}

pub async fn list_user_graphs(
    user_id: &str,
    pool: &sqlx::PgPool,
) -> Result<Vec<GraphListItem>, anyhow::Error> {
    let graphs: Vec<GraphListItem> = sqlx::query_as(
        r#"
    SELECT id, user_id, title, is_public, created_at, modified_at
    FROM graphs
    WHERE user_id = $1
    ORDER BY modified_at DESC
    LIMIT 10
    "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(graphs)
}
