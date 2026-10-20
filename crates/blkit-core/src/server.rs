use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde_json::{Value, json};

#[cfg(feature = "remote-persistence")]
use crate::distributed::DistributedControl;
#[cfg(all(feature = "worker", feature = "local-persistence"))]
use crate::runtime::Engine;

enum Backend {
    #[cfg(all(feature = "worker", feature = "local-persistence"))]
    Local(Arc<Engine>),
    #[cfg(feature = "remote-persistence")]
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

#[cfg(all(feature = "worker", feature = "local-persistence"))]
pub fn router(engine: Arc<Engine>) -> Router {
    routes(Backend::Local(engine))
}

#[cfg(feature = "remote-persistence")]
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
        .route("/instances/{id}/events", get(events))
        .route(
            "/instances/{id}/requests/{request_id}/response",
            post(respond),
        )
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
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.start(&namespace, &version, &name, input).await,
        #[cfg(feature = "remote-persistence")]
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
        Err(message) if message == "worker unavailable" => {
            self::error(StatusCode::SERVICE_UNAVAILABLE, &message)
        }
        Err(message) => internal("start", None, message),
    }
}

async fn status(State(engine): State<Arc<Backend>>, Path(id): Path<String>) -> Reply {
    let result = match engine.as_ref() {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.snapshot(&id).await.map(|snapshot| {
            snapshot.map(|(instance, requests)| local_snapshot(instance, requests))
        }),
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(control) => control
            .status(&id)
            .await
            .map(|item| item.map(|i| serde_json::to_value(i).unwrap())),
    };
    match result {
        Ok(Some(mut item)) => {
            item.as_object_mut().unwrap().remove("checkpoint");
            (StatusCode::OK, Json(item))
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "unknown instance"),
        Err(message) => internal("status", Some(&id), message),
    }
}

#[cfg(all(feature = "worker", feature = "local-persistence"))]
fn local_snapshot(
    instance: crate::store::Instance,
    requests: Vec<crate::compiled_graph::InputRequest>,
) -> Value {
    let mut item = serde_json::to_value(instance).unwrap();
    item.as_object_mut().unwrap().remove("checkpoint");
    item["requests"] = json!(requests);
    item
}

async fn events(State(backend): State<Arc<Backend>>, Path(id): Path<String>) -> Response {
    match backend.as_ref() {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => {
            let mut subscription = engine.subscribe(&id);
            let snapshot = match engine.snapshot(&id).await {
                Ok(Some((instance, requests))) => local_snapshot(instance, requests),
                Ok(None) => {
                    return error(StatusCode::NOT_FOUND, "unknown instance").into_response();
                }
                Err(message) => return internal("events", Some(&id), message).into_response(),
            };
            let stream = async_stream::stream! {
                yield Ok::<_, std::convert::Infallible>(Event::default().event("snapshot").data(snapshot.to_string()));
                // Lagged clients reconnect for a fresh snapshot.
                while let Ok(()) = subscription.recv().await {
                    yield Ok(Event::default().event("changed").data(json!({"id": id}).to_string()));
                }
            };
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(control) => {
            let (snapshot, mut subscription, registration) = match control.subscribe(&id).await {
                Ok(subscription) => subscription,
                Err(message) if message == "unknown instance" => {
                    return error(StatusCode::NOT_FOUND, &message).into_response();
                }
                Err(message) => return internal("events", Some(&id), message).into_response(),
            };
            let stream = async_stream::stream! {
                let _registration = registration;
                yield Ok::<_, std::convert::Infallible>(Event::default().event("snapshot").data(snapshot.to_string()));
                // A lagged client reconnects for a current snapshot.
                while let Ok(value) = subscription.recv().await {
                    let kind = if value.get("status").is_some() { "snapshot" } else { "changed" };
                    yield Ok(Event::default().event(kind).data(value.to_string()));
                }
            };
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
    }
}

async fn respond(
    State(engine): State<Arc<Backend>>,
    Path((id, request_id)): Path<(String, String)>,
    response: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Json(response) = match response {
        Ok(response) => response,
        Err(error) => return self::error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let result = match engine.as_ref() {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.respond(&id, &request_id, response).await,
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(control) => control.respond(&id, &request_id, response).await,
    };
    match result {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"id": id, "request_id": request_id, "status": "accepted"})),
        ),
        Err(message) if matches!(message.as_str(), "unknown instance" | "unknown request") => {
            error(StatusCode::NOT_FOUND, &message)
        }
        Err(message) if message.starts_with("invalid response:") => {
            error(StatusCode::BAD_REQUEST, &message)
        }
        Err(message)
            if matches!(
                message.as_str(),
                "request not outstanding"
                    | "instance not active"
                    | "request not outstanding or instance not active"
            ) =>
        {
            error(StatusCode::CONFLICT, &message)
        }
        Err(message) => internal("respond", Some(&id), message),
    }
}

async fn cancel(State(engine): State<Arc<Backend>>, Path(id): Path<String>) -> Reply {
    let result = match engine.as_ref() {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.cancel(&id).await,
        #[cfg(feature = "remote-persistence")]
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

#[cfg(all(test, feature = "logging"))]
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
