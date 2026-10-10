//! Operational logging for generated executables.
use std::{env, fs::OpenOptions, sync::Mutex, time::Duration};

use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, logs::SdkLoggerProvider};
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub use tracing;

/// Keep this value alive until process exit to flush the OTLP exporter.
pub struct Logging(Option<SdkLoggerProvider>);

impl Drop for Logging {
    fn drop(&mut self) {
        if let Some(provider) = &self.0 {
            let _ = provider.shutdown_with_timeout(Duration::from_secs(2));
        }
    }
}

pub fn init(service_name: &str) -> Result<Logging, String> {
    let level = match env::var("BLKIT_LOG_LEVEL") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => "info".into(),
        Err(_) => return Err("invalid BLKIT_LOG_LEVEL: not UTF-8".into()),
    };
    let filter = match level.to_ascii_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => return Err(format!("invalid BLKIT_LOG_LEVEL: {level}")),
    };
    let outputs = match env::var("BLKIT_LOG_OUTPUTS") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => "stdout".into(),
        Err(_) => return Err("invalid BLKIT_LOG_OUTPUTS: not UTF-8".into()),
    };
    let mut stdout = false;
    let mut file = false;
    let mut otlp = false;
    for output in outputs.split(',') {
        let selected = match output.trim() {
            "stdout" => &mut stdout,
            "file" => &mut file,
            "otlp" => &mut otlp,
            _ => return Err(format!("invalid BLKIT_LOG_OUTPUTS: {outputs}")),
        };
        if *selected {
            return Err(format!("duplicate BLKIT_LOG_OUTPUTS: {outputs}"));
        }
        *selected = true;
    }
    let file = if file {
        let path = env::var("BLKIT_LOG_FILE").map_err(|_| "file output requires BLKIT_LOG_FILE")?;
        if path.is_empty() {
            return Err("BLKIT_LOG_FILE must not be empty".into());
        }
        Some(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("BLKIT_LOG_FILE {path}: {e}"))?,
        ))
    } else {
        None
    };
    let provider = if otlp {
        let endpoint = env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
            .map_err(|_| "otlp output requires OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")?;
        let uri: http::Uri = endpoint
            .parse()
            .map_err(|_| "invalid OTEL_EXPORTER_OTLP_LOGS_ENDPOINT URL")?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().is_none()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        {
            return Err("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT must be an HTTP(S) logs URL".into());
        }
        let exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| format!("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT: {e}"))?;
        Some(
            SdkLoggerProvider::builder()
                .with_resource(
                    Resource::builder()
                        .with_service_name(service_name.to_owned())
                        .build(),
                )
                .with_batch_exporter(exporter)
                .build(),
        )
    } else {
        None
    };
    let otel_layer = provider.as_ref().map(OpenTelemetryTracingBridge::new);
    let console = stdout.then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stdout));
    let file_layer = file.map(|file| tracing_subscriber::fmt::layer().with_writer(file));
    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(file_layer)
        .with(otel_layer)
        .try_init()
        .map_err(|e| format!("logging subscriber: {e}"))?;
    Ok(Logging(provider))
}
