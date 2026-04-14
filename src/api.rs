use axum::{routing::post, Json, Router, extract::State};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use crate::uci::Uci;
use tower_http::cors::CorsLayer;

#[derive(Deserialize)]
pub struct UciRequest {
    pub command: String,
}

#[derive(Serialize)]
pub struct UciResponse {
    pub responses: Vec<String>,
}

pub async fn handle_uci_command(
    State(uci): State<Arc<Mutex<Uci>>>,
    Json(payload): Json<UciRequest>,
) -> Json<UciResponse> {
    let mut uci_lock = uci.lock().unwrap();
    let responses = uci_lock.process_command(&payload.command);
    Json(UciResponse { responses })
}

pub async fn run_server() {
    let uci_engine = Arc::new(Mutex::new(Uci::new()));

    let app = Router::new()
        .route("/uci", post(handle_uci_command))
        .layer(CorsLayer::permissive())
        .with_state(uci_engine);

    let addr = "127.0.0.1:3000";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Web server listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
