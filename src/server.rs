use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::{distributed::DistributedControl, runtime::Engine};

enum Backend {
    Local(Arc<Engine>),
    Distributed(Arc<DistributedControl>),
}

type Reply = (StatusCode, Json<Value>);

fn error(code: StatusCode, message: &str) -> Reply {
    (code, Json(json!({"error": message})))
}

fn internal(operation: &str, instance_id: Option<&str>, _error: String) -> Reply {
    tracing::error!(operation, instance_id, "runtime storage error");
    self::error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

pub fn router(engine: Arc<Engine>) -> Router {
    routes(Backend::Local(engine))
}

pub fn router_distributed(control: Arc<DistributedControl>) -> Router {
    routes(Backend::Distributed(control))
}

fn routes(backend: Backend) -> Router {
    Router::new()
        .route(
            "/processes/{namespace}/{version}/{name}/instances",
            post(start),
        )
        .route("/instances/{id}", get(status))
        .route("/instances/{id}/cancel", post(cancel))
        .with_state(Arc::new(backend))
}

async fn start(
    State(engine): State<Arc<Backend>>,
    Path((namespace, version, name)): Path<(String, String, String)>,
    input: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return self::error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let result = match engine.as_ref() {
        Backend::Local(engine) => engine.start(&namespace, &version, &name, input).await,
        Backend::Distributed(control) => control.start(&namespace, &version, &name, input).await,
    };
    match result {
        Ok(id) => (
            StatusCode::ACCEPTED,
            Json(json!({"id": id, "status": "pending"})),
        ),
        Err(message) if message == "unknown process" => {
            self::error(StatusCode::NOT_FOUND, &message)
        }
        Err(message) if message.starts_with("invalid input:") => {
            self::error(StatusCode::BAD_REQUEST, &message)
        }
        Err(message) => internal("start", None, message),
    }
}

async fn status(State(engine): State<Arc<Backend>>, Path(id): Path<String>) -> Reply {
    let result = match engine.as_ref() {
        Backend::Local(engine) => engine
            .status(&id)
            .await
            .map(|item| item.map(|i| serde_json::to_value(i).unwrap())),
        Backend::Distributed(control) => control
            .status(&id)
            .await
            .map(|item| item.map(|i| serde_json::to_value(i).unwrap())),
    };
    match result {
        Ok(Some(item)) => (StatusCode::OK, Json(item)),
        Ok(None) => error(StatusCode::NOT_FOUND, "unknown instance"),
        Err(message) => internal("status", Some(&id), message),
    }
}

async fn cancel(State(engine): State<Arc<Backend>>, Path(id): Path<String>) -> Reply {
    let result = match engine.as_ref() {
        Backend::Local(engine) => engine.cancel(&id).await,
        Backend::Distributed(control) => control.cancel(&id).await,
    };
    match result {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"id": id, "status": "cancelled"})),
        ),
        Err(message) if message == "unknown instance" => error(StatusCode::NOT_FOUND, &message),
        Err(message) if message == "instance already terminal" => {
            error(StatusCode::CONFLICT, &message)
        }
        Err(message) => internal("cancel", Some(&id), message),
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;

    #[test]
    fn storage_failure_uses_selected_log_sink_without_exposing_request_data() {
        let file =
            std::env::temp_dir().join(format!("blkit-server-events-{}.log", std::process::id()));
        let writer = std::sync::Mutex::new(std::fs::File::create(&file).unwrap());
        let subscriber = tracing_subscriber::fmt().with_writer(writer).finish();
        let reply = tracing::subscriber::with_default(subscriber, || {
            internal(
                "status",
                Some("instance-42"),
                "request-payload-secret".into(),
            )
        });
        assert_eq!(reply.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(reply.1.0["error"], "internal error");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("ERROR")
                && text.contains("storage")
                && text.contains("status")
                && text.contains("instance-42"),
            "{text}"
        );
        assert!(!text.contains("request-payload-secret"), "{text}");
        std::fs::remove_file(file).unwrap();
    }
}
