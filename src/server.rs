use std::sync::Arc;

use axum::{Json, Router, extract::{Path, State, rejection::JsonRejection}, http::StatusCode, routing::{get, post}};
use serde_json::{Value, json};

use crate::runtime::Engine;

type Reply = (StatusCode, Json<Value>);

fn error(code: StatusCode, message: &str) -> Reply {
    (code, Json(json!({"error": message})))
}

fn internal(error: String) -> Reply {
    eprintln!("runtime storage error: {error}");
    self::error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

pub fn router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route("/processes/{namespace}/{version}/{name}/instances", post(start))
        .route("/instances/{id}", get(status))
        .route("/instances/{id}/cancel", post(cancel))
        .with_state(engine)
}

async fn start(
    State(engine): State<Arc<Engine>>,
    Path((namespace, version, name)): Path<(String, String, String)>,
    input: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return self::error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    match engine.start(&namespace, &version, &name, input).await {
        Ok(id) => (StatusCode::ACCEPTED, Json(json!({"id": id, "status": "pending"}))),
        Err(message) if message == "unknown process" => self::error(StatusCode::NOT_FOUND, &message),
        Err(message) if message.starts_with("invalid input:") => self::error(StatusCode::BAD_REQUEST, &message),
        Err(message) => internal(message),
    }
}

async fn status(State(engine): State<Arc<Engine>>, Path(id): Path<String>) -> Reply {
    match engine.status(&id).await {
        Ok(Some(item)) => (StatusCode::OK, Json(serde_json::to_value(item).unwrap())),
        Ok(None) => error(StatusCode::NOT_FOUND, "unknown instance"),
        Err(message) => internal(message),
    }
}

async fn cancel(State(engine): State<Arc<Engine>>, Path(id): Path<String>) -> Reply {
    match engine.cancel(&id).await {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({"id": id, "status": "cancelled"}))),
        Err(message) if message == "unknown instance" => error(StatusCode::NOT_FOUND, &message),
        Err(message) if message == "instance already terminal" => error(StatusCode::CONFLICT, &message),
        Err(message) => internal(message),
    }
}
