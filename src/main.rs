use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    routing::{get, post},
};
use axum_macros::{FromRef, debug_handler};
use firebase_auth::{FirebaseAuth, FirebaseAuthState, FirebaseUser};
use futures::{StreamExt, stream};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder, key_extractor::KeyExtractor};
use tower_http::cors::{Any, CorsLayer};

use crate::database::get_graph_owner;

mod database;
mod llm;
mod serper;
mod types;

#[derive(FromRef, Clone)]
struct AppState {
    firebase_auth: FirebaseAuthState,
    client: reqwest::Client,
    pool: sqlx::PgPool,
}

#[derive(Clone)]
pub struct AuthTokenKeyExtractor;

impl KeyExtractor for AuthTokenKeyExtractor {
    type Key = String;

    fn extract<T>(
        &self,
        req: &axum::http::Request<T>,
    ) -> Result<Self::Key, tower_governor::GovernorError> {
        req.headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(|token| token.to_string())
            .ok_or(tower_governor::GovernorError::UnableToExtractKey)
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let firebase_auth = FirebaseAuth::new(&std::env::var("FIREBASE_PROJECT_ID").unwrap()).await;

    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();

    let app_state = AppState {
        firebase_auth: FirebaseAuthState::new(firebase_auth),
        client: reqwest::Client::new(),
        pool,
    };

    // Allow 1 request per second
    let secondly_conf = GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(3)
        .key_extractor(AuthTokenKeyExtractor)
        .finish()
        .unwrap();

    // Allow 50 requests per hour, but keep repleneshing every 72 seconds
    // So every 60 minutes, then get their 50 quota back
    let hourly_conf = GovernorConfigBuilder::default()
        .period(Duration::from_secs(72))
        .burst_size(50)
        .key_extractor(AuthTokenKeyExtractor)
        .finish()
        .unwrap();

    let origins = [
        "http://localhost:5173".parse().unwrap(),
        "https://recursio.app".parse().unwrap(),
    ];

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route(
            "/simple-query",
            post(simple_query).layer(GovernorLayer::new(hourly_conf)),
        )
        .route("/save-graph", post(save_graph))
        .route("/list-graphs", get(list_graphs))
        .route("/toggle-graph-visibility", post(toggle_graph_visibility))
        .layer(GovernorLayer::new(secondly_conf))
        // Get graph is below the Governor Layer so logged out users can access, we'll add IP throttling later
        .route("/get-graph", post(get_graph))
        .layer(cors)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:1337")
        .await
        .unwrap();

    let _ = axum::serve(listener, app).await;
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
}

async fn simple_query(
    State(app_state): State<AppState>,
    current_user: FirebaseUser,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<Vec<types::Node>>, (StatusCode, String)> {
    println!("Requesting: {}", payload.query);
    let llm_nodes = llm::simple_query(&payload.query, &app_state.client)
        .await
        .map_err(|e| {
            eprintln!("Error in llm::simple_query: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not use web search".to_string(),
            )
        })?;
    let nodes = map_llm_nodes_to_nodes(llm_nodes, &app_state.client).await;

    Ok(Json(nodes))
}

// Only called when the node data updates, we'll have other functions for title changes and public toggle
#[derive(Deserialize)]
struct SaveGraphRequest {
    graph_id: String,
    data: serde_json::Value,
}

async fn save_graph(
    State(app_state): State<AppState>,
    current_user: FirebaseUser,
    Json(payload): Json<SaveGraphRequest>,
) -> Result<(), (StatusCode, String)> {
    let updated = database::update_graph_if_owner(
        &payload.graph_id,
        &current_user.user_id,
        &payload.data,
        &app_state.pool,
    )
    .await
    .map_err(|e| {
        eprintln!("Error in database::update_graph_if_owner: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not update graph".to_string(),
        )
    })?;
    if updated {
        return Ok(());
    }
    // We need to insert a new graph, it will insert a new graph if it doesn't already exists.
    // Defaults to private graph
    let inserted = database::insert_new_graph(
        &payload.graph_id,
        &current_user.user_id,
        &payload.data,
        &app_state.pool,
    )
    .await
    .map_err(|e| {
        eprintln!("Error in database::insert_new_graph: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save your graph".to_string(),
        )
    })?;
    if inserted {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "This graph already exists!".to_string(),
        ))
    }
}

#[derive(Deserialize)]
struct GetGraphRequest {
    graph_id: String,
}

#[derive(Deserialize)]
struct GraphVisibilityToggleRequest {
    graph_id: String,
    is_public: bool,
}

pub struct OptionalUser(pub Option<FirebaseUser>);

impl<S> FromRequestParts<S> for OptionalUser
where
    S: Send + Sync,
    FirebaseUser: FromRequestParts<S>,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match FirebaseUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalUser(Some(user))),
            Err(_) => Ok(OptionalUser(None)),
        }
    }
}

async fn get_graph(
    State(app_state): State<AppState>,
    current_user: OptionalUser,
    Json(payload): Json<GetGraphRequest>,
) -> Result<Json<database::GraphItem>, (StatusCode, String)> {
    match current_user.0 {
        // User is logged in
        Some(user) => {
            println!("User is logged in!");
            // Check if user owns the graph
            let graph_owner = database::get_graph_owner(&payload.graph_id, &app_state.pool)
                .await
                .map_err(|e| {
                    eprintln!("Error in database::get_graph_owner: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not get the graph!".to_string(),
                    )
                })?;

            if graph_owner == user.user_id {
                println!("User owns the graph!");
                // User owns the graph
                let res =
                    database::get_graph_data(&payload.graph_id, &user.user_id, &app_state.pool)
                        .await
                        .map_err(|e| {
                            eprintln!("Error in database::get_graph_data: {}", e);
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Could not get graph".to_string(),
                            )
                        })?;
                Ok(Json(res))
            } else {
                println!("User doesn't own the graph!");
                let res = database::get_graph_public(&payload.graph_id, &app_state.pool)
                    .await
                    .map_err(|e| {
                        eprintln!("Error in database::get_graph_public: {}", e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Could not get graph".to_string(),
                        )
                    })?;
                return Ok(Json(res));
            }
        }
        // User isn't logged in
        None => {
            println!("User is logged out!");
            let res = database::get_graph_public(&payload.graph_id, &app_state.pool)
                .await
                .map_err(|e| {
                    eprintln!("Error in database::get_graph_public: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not get graph".to_string(),
                    )
                })?;
            return Ok(Json(res));
        }
    }
}

#[debug_handler]
async fn toggle_graph_visibility(
    State(app_state): State<AppState>,
    current_user: FirebaseUser,
    Json(payload): Json<GraphVisibilityToggleRequest>,
) -> Result<(), (StatusCode, String)> {
    let toggled = database::toggle_graph_visibility(
        &payload.graph_id,
        &current_user.user_id,
        payload.is_public,
        &app_state.pool,
    )
    .await
    .map_err(|e| {
        eprintln!("Error in database::toggle_graph_visibility: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not toggle visibility".to_string(),
        )
    })?;
    if toggled {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "You can't change the visibility of this graph!".to_string(),
        ))
    }
}

#[derive(Serialize)]
struct ListGraphsResponse {
    graphs: Vec<database::GraphListItem>,
}

async fn list_graphs(
    State(app_state): State<AppState>,
    current_user: FirebaseUser,
) -> Result<Json<ListGraphsResponse>, (StatusCode, String)> {
    let res = database::list_user_graphs(&current_user.user_id, &app_state.pool)
        .await
        .map_err(|e| {
            eprintln!("Error in database::get_graph_data: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not get graph".to_string(),
            )
        })?;
    Ok(Json(ListGraphsResponse { graphs: res }))
}

async fn map_llm_nodes_to_nodes(
    nodes: Vec<llm::LLMNode>,
    client: &reqwest::Client,
) -> Vec<types::Node> {
    stream::iter(nodes)
        .map(|llm_node| async move {
            let mut image = None;
            if let Some(query) = llm_node.image_query {
                if let Ok(result) = serper::search_images(&query, client).await {
                    image = result.images.get(0).map(|img| types::Image {
                        url: img.thumbnail_url.clone().unwrap_or_default(),
                        link: img.link.clone().unwrap_or_default(),
                    })
                }
            }
            types::Node {
                title: llm_node.title,
                description: llm_node.description,
                image: image,
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await
}
