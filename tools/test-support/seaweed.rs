use std::process::Command;
use testcontainers_modules::testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

// Keep the container alive for as long as callers need the endpoint.
pub async fn start() -> (
    testcontainers_modules::testcontainers::ContainerAsync<GenericImage>,
    String,
) {
    let container = GenericImage::new("chrislusf/seaweedfs", "4.48")
        .with_exposed_port(8333.tcp())
        .with_wait_for(WaitFor::message_on_stdout("S3 Endpoint:"))
        .with_env_var("AWS_ACCESS_KEY_ID", "admin")
        .with_env_var("AWS_SECRET_ACCESS_KEY", "secret")
        .with_env_var("S3_BUCKET", "my-bucket")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(container.get_host().await.unwrap().to_string());
    let endpoint = format!(
        "http://{host}:{}",
        container.get_host_port_ipv4(8333).await.unwrap()
    );
    (container, endpoint)
}

pub fn configure(command: &mut Command, endpoint: &str) {
    command
        .env("BLKIT_ATTACHMENT_BACKEND", "s3")
        .env("BLKIT_ATTACHMENT_BUCKET", "my-bucket")
        .env("AWS_ACCESS_KEY_ID", "admin")
        .env("AWS_SECRET_ACCESS_KEY", "secret")
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .env("AWS_ENDPOINT", endpoint)
        .env_remove("AWS_ENDPOINT_URL_S3")
        .env("AWS_ALLOW_HTTP", "true");
}
