use std::sync::Arc;

#[cfg(feature = "remote-persistence")]
use crate::distributed::DistributedControl;
#[cfg(all(feature = "worker", feature = "local-persistence"))]
use crate::runtime::Engine;
#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
use crate::{attachment_store::AttachmentStore, store::Attachment};
#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
use axum::extract::{DefaultBodyLimit, Multipart};
use axum::{
    Json, Router,
    extract::{FromRequest, Path, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde_json::{Value, json};

enum Backend {
    #[cfg(all(feature = "worker", feature = "local-persistence"))]
    Local(Arc<Engine>),
    #[cfg(feature = "remote-persistence")]
    Distributed(Arc<DistributedControl>),
}

struct ServerState {
    backend: Backend,
    #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
    attachments: Option<AttachmentStore>,
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
    routes(
        Backend::Local(engine),
        #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
        None,
    )
}

#[cfg(feature = "remote-persistence")]
pub fn router_distributed(control: Arc<DistributedControl>) -> Router {
    routes(
        Backend::Distributed(control),
        #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
        None,
    )
}

#[cfg(all(feature = "worker", feature = "local-persistence"))]
#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
pub fn router_with_attachments(engine: Arc<Engine>, attachments: AttachmentStore) -> Router {
    routes(Backend::Local(engine), Some(attachments))
}

#[cfg(feature = "remote-persistence")]
#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
pub fn router_distributed_with_attachments(
    control: Arc<DistributedControl>,
    attachments: AttachmentStore,
) -> Router {
    routes(Backend::Distributed(control), Some(attachments))
}

fn routes(
    backend: Backend,
    #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))] attachments: Option<
        AttachmentStore,
    >,
) -> Router {
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
        .with_state(Arc::new(ServerState {
            backend,
            #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
            attachments,
        }))
}

async fn start(
    State(engine): State<Arc<ServerState>>,
    Path((namespace, version, name)): Path<(String, String, String)>,
    request: Request,
) -> Reply {
    let multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("multipart/form-data"))
        });
    if multipart {
        #[cfg(not(any(feature = "attachment-local", feature = "attachment-cloud")))]
        return self::error(StatusCode::BAD_REQUEST, "multipart attachments unavailable");
        #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
        return start_multipart(engine, &namespace, &version, &name, request).await;
    }
    let input: Result<Json<Value>, JsonRejection> = Json::from_request(request, &()).await;
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return self::error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let result = match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.start(&namespace, &version, &name, input).await,
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(control) => control.start(&namespace, &version, &name, input).await,
    };
    start_result(result)
}

#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
async fn start_multipart(
    engine: Arc<ServerState>,
    namespace: &str,
    version: &str,
    name: &str,
    mut request: Request,
) -> Reply {
    let Some(objects) = engine.attachments.as_ref() else {
        return self::error(StatusCode::BAD_REQUEST, "multipart attachments unavailable");
    };
    // Preflight without reserving a DB connection during streaming; persistence checks again.
    match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(local) => {
            if !local.has_process(&namespace, &version, &name) {
                return self::error(StatusCode::NOT_FOUND, "unknown process");
            }
        }
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(remote) => match remote.can_start(&namespace, &version, &name).await {
            Ok(true) => {}
            Ok(false) => return self::error(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable"),
            Err(message) => return internal("start", None, message),
        },
    }
    DefaultBodyLimit::max(crate::multipart::MAX_BODY_BYTES).apply(&mut request);
    let multipart = match Multipart::from_request(request, &()).await {
        Ok(value) => value,
        Err(rejection) => return self::error(StatusCode::BAD_REQUEST, &rejection.to_string()),
    };
    let staged = match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(local) => {
            crate::multipart::decode_validated(multipart, "input", objects, |input| {
                let result = local
                    .validate_start_input(namespace, version, name, input)
                    .map_err(|_| StatusCode::BAD_REQUEST);
                Box::pin(async move { result })
            })
            .await
        }
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(_) => crate::multipart::decode(multipart, "input", objects).await,
    };
    let staged = match staged {
        Ok(staged) => staged,
        Err(code) => return self::error(code, "invalid multipart start"),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let attachments: Vec<_> = staged
        .files
        .iter()
        .enumerate()
        .map(|(ordinal, file)| Attachment {
            instance_id: id.clone(),
            request_id: None,
            ordinal: ordinal as i64,
            object_key: file.key.to_string(),
            filename: file.filename.clone(),
            media_type: file.media_type.clone(),
            byte_length: file.byte_length,
        })
        .collect();
    let result = match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(local) => {
            local
                .start_with_attachments(
                    id.clone(),
                    &namespace,
                    &version,
                    &name,
                    staged.value.clone(),
                    &attachments,
                )
                .await
        }
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(remote) => {
            remote
                .start_with_attachments(
                    id.clone(),
                    &namespace,
                    &version,
                    &name,
                    staged.value.clone(),
                    &attachments,
                )
                .await
        }
    };
    if let Err(message) = &result {
        let persisted = match &engine.backend {
            #[cfg(all(feature = "worker", feature = "local-persistence"))]
            Backend::Local(local) => local.start_attachments(&id).await,
            #[cfg(feature = "remote-persistence")]
            Backend::Distributed(remote) => remote.start_attachments(&id).await,
        };
        match persisted {
            Ok(committed) if start_failure_can_cleanup(message, &committed, &attachments) => {
                if let Err(error) = staged.cleanup(objects).await {
                    tracing::warn!(%error, "failed start cleanup failed");
                }
            }
            Ok(_) => {
                tracing::warn!(instance_id = %id, "start commit may have persisted; retaining uploaded objects")
            }
            Err(error) => {
                tracing::warn!(%error, instance_id = %id, "cannot verify failed start; retaining uploaded objects")
            }
        }
    }
    start_result(result)
}

#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
fn start_failure_can_cleanup(
    message: &str,
    committed: &[Attachment],
    uploaded: &[Attachment],
) -> bool {
    !message.starts_with(crate::store::UNCERTAIN_START_COMMIT)
        && !committed.iter().any(|file| {
            uploaded
                .iter()
                .any(|staged| staged.object_key == file.object_key)
        })
}

fn start_result(result: Result<String, String>) -> Reply {
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

async fn status(State(engine): State<Arc<ServerState>>, Path(id): Path<String>) -> Reply {
    let result = match &engine.backend {
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

async fn events(State(backend): State<Arc<ServerState>>, Path(id): Path<String>) -> Response {
    match &backend.backend {
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
    State(engine): State<Arc<ServerState>>,
    Path((id, request_id)): Path<(String, String)>,
    request: Request,
) -> Reply {
    let multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("multipart/form-data"))
        });
    if multipart {
        #[cfg(not(any(feature = "attachment-local", feature = "attachment-cloud")))]
        return self::error(StatusCode::BAD_REQUEST, "multipart attachments unavailable");
        #[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
        return respond_multipart(engine, &id, &request_id, request).await;
    }
    let response: Result<Json<Value>, JsonRejection> = Json::from_request(request, &()).await;
    let Json(response) = match response {
        Ok(response) => response,
        Err(error) => return self::error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let result = match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(engine) => engine.respond(&id, &request_id, response).await,
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(control) => control.respond(&id, &request_id, response).await,
    };
    respond_result(&id, &request_id, result)
}

#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
async fn respond_multipart(
    engine: Arc<ServerState>,
    id: &str,
    request_id: &str,
    mut request: Request,
) -> Reply {
    let Some(objects) = engine.attachments.as_ref() else {
        return self::error(StatusCode::BAD_REQUEST, "multipart attachments unavailable");
    };
    DefaultBodyLimit::max(crate::multipart::MAX_BODY_BYTES).apply(&mut request);
    let multipart = match Multipart::from_request(request, &()).await {
        Ok(value) => value,
        Err(rejection) => return self::error(StatusCode::BAD_REQUEST, &rejection.to_string()),
    };
    let staged = crate::multipart::decode_validated(multipart, "response", objects, |response| {
        let engine = engine.clone();
        let id = id.to_owned();
        let request_id = request_id.to_owned();
        Box::pin(async move {
            let result = match &engine.backend {
                #[cfg(all(feature = "worker", feature = "local-persistence"))]
                Backend::Local(local) => local.validate_reply(&id, &request_id, response).await,
                #[cfg(feature = "remote-persistence")]
                Backend::Distributed(remote) => {
                    remote.validate_reply(&id, &request_id, response).await
                }
            };
            result.map_err(|message| respond_error(&id, message).0)
        })
    })
    .await;
    let staged = match staged {
        Ok(staged) => staged,
        Err(code) => return self::error(code, "invalid multipart response"),
    };
    let attachments: Vec<_> = staged
        .files
        .iter()
        .enumerate()
        .map(|(ordinal, file)| Attachment {
            instance_id: id.to_owned(),
            request_id: Some(request_id.to_owned()),
            ordinal: ordinal as i64,
            object_key: file.key.to_string(),
            filename: file.filename.clone(),
            media_type: file.media_type.clone(),
            byte_length: file.byte_length,
        })
        .collect();
    let result = match &engine.backend {
        #[cfg(all(feature = "worker", feature = "local-persistence"))]
        Backend::Local(local) => {
            local
                .respond_with_attachments(id, request_id, staged.value.clone(), &attachments)
                .await
        }
        #[cfg(feature = "remote-persistence")]
        Backend::Distributed(remote) => {
            remote
                .respond_with_attachments(id, request_id, staged.value.clone(), &attachments)
                .await
        }
    };
    if let Err(message) = &result {
        let committed = match &engine.backend {
            #[cfg(all(feature = "worker", feature = "local-persistence"))]
            Backend::Local(local) => local.reply_attachments(id, request_id).await,
            #[cfg(feature = "remote-persistence")]
            Backend::Distributed(remote) => remote.reply_attachments(id, request_id).await,
        };
        match committed {
            Ok(files)
                if !message.starts_with(crate::store::UNCERTAIN_REPLY_COMMIT)
                    && !files.iter().any(|file| {
                        attachments
                            .iter()
                            .any(|uploaded| uploaded.object_key == file.object_key)
                    }) =>
            {
                if let Err(error) = staged.cleanup(objects).await {
                    tracing::warn!(%error, "failed reply cleanup failed");
                }
            }
            Ok(_) => {
                tracing::warn!(instance_id = %id, "reply commit may have persisted; retaining uploaded objects")
            }
            Err(error) => {
                tracing::warn!(%error, instance_id = %id, "cannot verify failed reply; retaining uploaded objects")
            }
        }
    }
    respond_result(id, request_id, result)
}

fn respond_result(id: &str, request_id: &str, result: Result<(), String>) -> Reply {
    match result {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"id": id, "request_id": request_id, "status": "accepted"})),
        ),
        Err(message) => respond_error(id, message),
    }
}

fn respond_error(id: &str, message: String) -> Reply {
    if matches!(message.as_str(), "unknown instance" | "unknown request") {
        error(StatusCode::NOT_FOUND, &message)
    } else if message.starts_with("invalid response:") {
        error(StatusCode::BAD_REQUEST, &message)
    } else if matches!(
        message.as_str(),
        "request not outstanding"
            | "instance not active"
            | "request not outstanding or instance not active"
    ) {
        error(StatusCode::CONFLICT, &message)
    } else {
        internal("respond", Some(id), message)
    }
}

async fn cancel(State(engine): State<Arc<ServerState>>, Path(id): Path<String>) -> Reply {
    let result = match &engine.backend {
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

#[cfg(all(test, feature = "attachment-local"))]
mod start_cleanup_tests {
    use super::*;

    #[test]
    fn uncertain_commit_never_cleans_objects_on_an_empty_read() {
        assert!(!start_failure_can_cleanup(
            "uncertain start commit: connection lost",
            &[],
            &[]
        ));
        assert!(start_failure_can_cleanup(
            "injected metadata failure",
            &[],
            &[]
        ));
        let file = Attachment {
            instance_id: "one".into(),
            request_id: None,
            ordinal: 0,
            object_key: "objects/one".into(),
            filename: "one.txt".into(),
            media_type: None,
            byte_length: 1,
        };
        assert!(!start_failure_can_cleanup(
            "injected metadata failure",
            &[file.clone()],
            &[file]
        ));
    }

    #[cfg(all(feature = "worker", feature = "local-persistence"))]
    #[tokio::test]
    async fn a_committed_start_association_survives_an_injected_lost_ack() {
        use crate::runtime::{Instance, LocalStore};
        use object_store::ObjectStoreExt;
        let root = std::env::temp_dir().join(format!("blkit-ambiguous-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let db = root.join("db");
        let metadata = LocalStore::open(&db).await.unwrap();
        let objects = AttachmentStore::local(&db, None).await.unwrap();
        let key = AttachmentStore::new_key();
        objects
            .object_store()
            .put(&key, "contents".into())
            .await
            .unwrap();
        let file = Attachment {
            instance_id: "committed".into(),
            request_id: None,
            ordinal: 0,
            object_key: key.to_string(),
            filename: "a.txt".into(),
            media_type: None,
            byte_length: 8,
        };
        metadata
            .create_with_attachments(
                &Instance::new("committed", "demo", "1", "p", Value::Null),
                &[file.clone()],
            )
            .await
            .unwrap();
        let committed = metadata.attachments("committed", None).await.unwrap();
        // Simulate losing the COMMIT acknowledgement after the database made the association visible.
        if start_failure_can_cleanup(
            "uncertain start commit: injected lost acknowledgement",
            &committed,
            &[file],
        ) {
            objects.object_store().delete(&key).await.unwrap();
        }
        assert_eq!(
            objects
                .object_store()
                .get(&key)
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
                .as_ref(),
            b"contents"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
