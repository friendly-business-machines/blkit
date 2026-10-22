use blkit_transpiler::project::Project;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};

#[path = "../../../tools/test-support/seaweed.rs"]
mod seaweed;

const SOURCE: &str = r#"namespace orders; version "1";
start_event start { output amount: Number; }
input_request review { prompt "Approve?"; output approved: Bool; }
end_event done { input result: Bool; }
process route {
  flow start -> review;
  flow review -> done;
  bind review.approved -> done.result;
}"#;

// Compiled in the generated worker role, not in the API role: it uses the same
// worker-only core features and independently opens durable metadata and objects.
const LOCAL_READER: &str = r#"
use blkit_core::{attachment_reader::AttachmentReader, attachment_store::AttachmentStore, runtime::LocalStore};
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = AttachmentReader::local(
        LocalStore::open(std::path::Path::new(&args[1])).await.unwrap(),
        AttachmentStore::local(std::path::Path::new(&args[1]), None).await.unwrap(),
    );
    let scope = (args[3] != "-").then_some(args[3].as_str());
    let mut files = Vec::new();
    for entry in reader.list(&args[2], scope).await.unwrap() {
        let bytes = reader.read(&args[2], scope, entry.ordinal).await.unwrap().unwrap();
        files.push(serde_json::json!({"ordinal": entry.ordinal, "filename": entry.filename,
            "media_type": entry.media_type, "byte_length": entry.byte_length, "bytes": bytes}));
    }
    println!("{}", serde_json::to_string(&files).unwrap());
}
"#;
const REMOTE_READER: &str = r#"
use blkit_core::{attachment_reader::AttachmentReader, attachment_store::{AttachmentStore, AttachmentRole}, postgres_store::PostgresStore};
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = AttachmentReader::remote(
        PostgresStore::connect(&args[1]).await.unwrap(),
        AttachmentStore::remote_from_env(AttachmentRole::Worker).await.unwrap(),
    );
    let scope = (args[3] != "-").then_some(args[3].as_str());
    let mut files = Vec::new();
    for entry in reader.list(&args[2], scope).await.unwrap() {
        let bytes = reader.read(&args[2], scope, entry.ordinal).await.unwrap().unwrap();
        files.push(serde_json::json!({"ordinal": entry.ordinal, "filename": entry.filename,
            "media_type": entry.media_type, "byte_length": entry.byte_length, "bytes": bytes}));
    }
    println!("{}", serde_json::to_string(&files).unwrap());
}
"#;

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn binary(root: &Path, name: &str) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".blkit/target"))
        .join("debug")
        .join(name)
}

#[test]
fn generated_binary_path_without_target_dir_uses_project_target() {
    if std::env::var_os("BLKIT_TEST_UNSET_TARGET").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "generated_binary_path_without_target_dir_uses_project_target",
            ])
            .env_remove("CARGO_TARGET_DIR")
            .env("BLKIT_TEST_UNSET_TARGET", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let root = Path::new("/tmp/blkit-generated-path");
    assert_eq!(
        binary(root, "orders-api-worker"),
        root.join(".blkit/target/debug/orders-api-worker")
    );
}

fn generate(root: &Path, name: &str, target: &str, reader: &str, local: bool) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("blkit.toml"),
        format!(
            "[project]\nname = {name:?}\nblkit = \"0.1.0\"\nbuild_target = {target:?}\n{}",
            if target == "api-worker" {
                if local {
                    "persistence = \"local\"\n"
                } else {
                    "persistence = \"remote\"\n"
                }
            } else {
                ""
            }
        ),
    )
    .unwrap();
    fs::write(root.join("route.bl"), SOURCE).unwrap();
    Project::load(root).unwrap().transpile().unwrap();
    fs::write(
        root.join(format!(".blkit/src/bin/{name}-attachment-inspect.rs")),
        reader,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(root.join(".blkit/Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if target == "api-worker-split" {
        let output = Command::new("cargo")
            .args(["build", "--manifest-path"])
            .arg(root.join(".blkit/api/Cargo.toml"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn launch(root: &Path, name: &str, args: &[&str], cloud: Option<&str>) -> Server {
    let mut command = Command::new(binary(root, name));
    command
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(endpoint) = cloud {
        seaweed::configure(&mut command, endpoint);
    }
    Server(command.spawn().unwrap())
}

fn request(port: u16, path: &str, fields: &[String]) -> (u16, String) {
    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "--max-time",
        "5",
        "-w",
        "\n%{http_code}",
        "-X",
        "POST",
    ]);
    for field in fields {
        command.args(["-F", field]);
    }
    let output = command
        .arg(format!("http://127.0.0.1:{port}{path}"))
        .output()
        .unwrap();
    if !output.status.success() {
        return (0, String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let text = String::from_utf8(output.stdout).unwrap();
    let (body, code) = text.rsplit_once('\n').unwrap();
    (code.parse().unwrap(), body.into())
}

fn status(port: u16, id: &str) -> Value {
    let url = format!("http://127.0.0.1:{port}/instances/{id}");
    let output = Command::new("curl")
        .args(["-fsS", "--max-time", "5", &url])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn until(mut check: impl FnMut() -> Option<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(item) = check() {
                return item;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("instance did not reach expected state")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn inspect(
    root: &Path,
    name: &str,
    database: &str,
    id: &str,
    scope: &str,
    cloud: Option<&str>,
) -> Value {
    let mut command = Command::new(binary(root, &format!("{name}-attachment-inspect")));
    command.args([database, id, scope]);
    if let Some(endpoint) = cloud {
        seaweed::configure(&mut command, endpoint);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn local_db_references_only(database: &Path, id: &str, request_id: &str, secrets: &[&str]) {
    let db = turso::Builder::new_local(database.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn.query("SELECT request_id, ordinal, object_key, filename, byte_length FROM instance_attachments WHERE instance_id=?1 ORDER BY request_id, ordinal", [id]).await.unwrap();
    let mut found = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        let key: String = row.get(2).unwrap();
        assert!(key.starts_with("blkit/attachments/") && !key.contains(".txt"));
        found.push((
            row.get::<Option<String>>(0).unwrap(),
            row.get::<i64>(1).unwrap(),
            row.get::<String>(3).unwrap(),
            row.get::<i64>(4).unwrap(),
        ));
    }
    assert_eq!(
        found,
        vec![
            (None, 0, "first.txt".into(), 15),
            (None, 1, "second.txt".into(), 16),
            (Some(request_id.into()), 0, "reply.txt".into(), 15)
        ]
    );
    let mut rows = conn
        .query(
            "SELECT name FROM pragma_table_info('instance_attachments')",
            (),
        )
        .await
        .unwrap();
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        columns.push(row.get::<String>(0).unwrap());
    }
    assert_eq!(
        columns,
        [
            "instance_id",
            "request_id",
            "ordinal",
            "object_key",
            "filename",
            "media_type",
            "byte_length"
        ]
    );
    let mut rows = conn
        .query("SELECT input, checkpoint FROM instances WHERE id=?1", [id])
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let text = format!(
        "{:?}{:?}",
        row.get::<String>(0).unwrap(),
        row.get::<Option<String>>(1).unwrap()
    );
    for secret in secrets {
        assert!(!text.contains(secret), "DB instance stores file bytes");
    }
    let mut rows = conn
        .query(
            "SELECT response FROM input_requests WHERE instance_id=?1",
            [id],
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let response: Option<String> = row.get(0).unwrap();
    for secret in secrets {
        assert!(!response.as_deref().unwrap_or("").contains(secret));
    }
}

async fn remote_db_references_only(url: &str, id: &str, request_id: &str, secrets: &[&str]) {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    let rows = client.query("SELECT request_id, ordinal, object_key, filename, byte_length FROM instance_attachments WHERE instance_id=$1 ORDER BY request_id NULLS FIRST, ordinal", &[&id]).await.unwrap();
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        let key: String = row.get(2);
        assert!(key.starts_with("blkit/attachments/") && !key.contains(".txt"));
        let expected = match index {
            0 => (None, 0, "first.txt", 15),
            1 => (None, 1, "second.txt", 16),
            _ => (Some(request_id.to_owned()), 0, "reply.txt", 15),
        };
        assert_eq!(
            (
                row.get::<_, Option<String>>(0),
                row.get::<_, i64>(1),
                row.get::<_, String>(3),
                row.get::<_, i64>(4)
            ),
            (expected.0, expected.1, expected.2.to_owned(), expected.3)
        );
    }
    let columns: Vec<String> = client.query("SELECT column_name FROM information_schema.columns WHERE table_name='instance_attachments' ORDER BY ordinal_position", &[]).await.unwrap().iter().map(|row| row.get(0)).collect();
    assert_eq!(
        columns,
        [
            "instance_id",
            "request_id",
            "ordinal",
            "object_key",
            "filename",
            "media_type",
            "byte_length"
        ]
    );
    let rows = client.query("SELECT row_to_json(t)::text FROM (SELECT input, checkpoint FROM instances WHERE id=$1) t", &[&id]).await.unwrap();
    let reply = client
        .query_one(
            "SELECT response FROM input_requests WHERE instance_id=$1 AND request_id=$2",
            &[&id, &request_id],
        )
        .await
        .unwrap();
    let text = format!(
        "{}{:?}",
        rows[0].get::<_, String>(0),
        reply.get::<_, Option<String>>(0)
    );
    for secret in secrets {
        assert!(!text.contains(secret), "DB stores file bytes");
    }
}

async fn scenario(root: &Path, database: &str, name: &str, target: &str, cloud: Option<&str>) {
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let is_local = cloud.is_none();
    let mut worker = if target == "api-worker-split" {
        Some(launch(
            root,
            &format!("{name}-worker"),
            &[database, "2", "1000"],
            cloud,
        ))
    } else {
        None
    };
    let server_name = if target == "api-worker-split" {
        format!("{name}-api")
    } else {
        format!("{name}-api-worker")
    };
    let args: Vec<&str> = if target == "api-worker-split" {
        vec![database, &bind]
    } else if is_local {
        vec![database, "2", &bind]
    } else {
        vec![database, "2", "1000", &bind]
    };
    let mut server = Some(launch(root, &server_name, &args, cloud));
    let first = root.join("first.txt");
    let second = root.join("second.txt");
    let reply = root.join("reply.txt");
    let secrets = ["start-secret-A!", "start-secret-B!!", "reply-secret-C!"];
    for (path, text) in [
        (&first, secrets[0]),
        (&second, secrets[1]),
        (&reply, secrets[2]),
    ] {
        fs::write(path, text).unwrap();
    }
    let start_path = "/processes/orders/1/route/instances";
    let fields = [
        "input={\"amount\":\"3\"};type=application/json".to_owned(),
        format!(
            "files=@{};filename=first.txt;type=text/plain",
            first.display()
        ),
        format!(
            "files=@{};filename=second.txt;type=text/plain",
            second.display()
        ),
    ];
    let accepted = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let (code, body) = request(port, start_path, &fields);
            if code == 202 {
                break serde_json::from_str::<Value>(&body).unwrap();
            }
            assert!(matches!(code, 0 | 503), "unexpected start {code}: {body}");
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("generated role did not accept multipart start");
    let id = accepted["id"].as_str().unwrap();
    let waiting = until(|| {
        let row = status(port, id);
        (row["status"] == "waiting-for-input").then_some(row)
    })
    .await;
    let request_id = waiting["requests"][0]["id"].as_str().unwrap();
    assert_eq!(waiting["input"], json!({"amount":"3"}));
    drop(worker.take());
    if target != "api-worker-split" {
        drop(server.take());
        server = Some(launch(root, &server_name, &args, cloud));
    }
    if target == "api-worker-split" {
        worker = Some(launch(
            root,
            &format!("{name}-worker"),
            &[database, "2", "1000"],
            cloud,
        ));
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let output = Command::new("curl")
                .args([
                    "-sS",
                    "--max-time",
                    "2",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                ])
                .arg(format!("http://127.0.0.1:{port}/instances/{id}"))
                .output()
                .unwrap();
            if output.stdout == b"200" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("restarted API did not listen");
    let expected_start = json!([
        {"ordinal":0,"filename":"first.txt","media_type":"text/plain","byte_length":15,"bytes": secrets[0].as_bytes()},
        {"ordinal":1,"filename":"second.txt","media_type":"text/plain","byte_length":16,"bytes": secrets[1].as_bytes()}
    ]);
    if is_local {
        drop(server.take());
    }
    assert_eq!(
        inspect(root, name, database, id, "-", cloud),
        expected_start
    );
    if is_local {
        server = Some(launch(root, &server_name, &args, cloud));
        tokio::time::timeout(Duration::from_secs(10), async {
            while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("local API did not restart before SSE subscription");
    }
    let events_file = root.join("events.txt");
    let events = Server(
        Command::new("curl")
            .arg("-sN")
            .arg(format!("http://127.0.0.1:{port}/instances/{id}/events"))
            .stdout(Stdio::from(fs::File::create(&events_file).unwrap()))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let text = fs::read_to_string(&events_file).unwrap();
            if text.contains("event: snapshot") && text.contains("waiting-for-input") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("SSE did not subscribe while waiting for input");
    let reply_path = format!("/instances/{id}/requests/{request_id}/response");
    let reply_fields = [
        "response={\"approved\":true};type=application/json".to_owned(),
        format!(
            "files=@{};filename=reply.txt;type=text/plain",
            reply.display()
        ),
    ];
    let (code, body) = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let result = request(port, &reply_path, &reply_fields);
            if result.0 != 0 {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("restarted API did not listen");
    assert_eq!(code, 202, "{body}");
    let completed = until(|| {
        let row = status(port, id);
        (row["status"] == "completed").then_some(row)
    })
    .await;
    assert_eq!(completed["result"], true);
    let post_reply_events = Command::new("curl")
        .args(["-sN", "--max-time", "10"])
        .arg(format!("http://127.0.0.1:{port}/instances/{id}/events"))
        .output()
        .unwrap();
    let post_reply_sse = String::from_utf8(post_reply_events.stdout).unwrap();
    let mut lines = post_reply_sse.lines();
    assert_eq!(
        lines.find(|line| line.starts_with("event: ")),
        Some("event: snapshot"),
        "{post_reply_sse}"
    );
    let post_reply_snapshot: Value = serde_json::from_str(
        lines
            .find_map(|line| line.strip_prefix("data: "))
            .expect("missing post-reply SSE snapshot"),
    )
    .unwrap();
    assert_eq!(post_reply_snapshot["status"], "completed");
    let sse = fs::read_to_string(&events_file).unwrap();
    drop(events);
    let waiting_text = waiting.to_string();
    let completed_text = completed.to_string();
    for secret in secrets {
        assert!(
            !waiting_text.contains(secret)
                && !completed_text.contains(secret)
                && !sse.contains(secret)
                && !post_reply_sse.contains(secret)
        );
    }
    if is_local {
        drop(server.take());
    }
    assert_eq!(
        inspect(root, name, database, id, "-", cloud),
        expected_start
    );
    assert_eq!(
        inspect(root, name, database, id, request_id, cloud),
        json!([{"ordinal":0,"filename":"reply.txt","media_type":"text/plain","byte_length":15,"bytes":secrets[2].as_bytes()}])
    );
    let keys = if is_local {
        local_db_references_only(Path::new(database), id, request_id, &secrets).await;
        let db = turso::Builder::new_local(database).build().await.unwrap();
        let conn = db.connect().unwrap();
        let mut rows = conn
            .query(
                "SELECT object_key FROM instance_attachments WHERE instance_id=?1",
                [id],
            )
            .await
            .unwrap();
        let mut keys = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            keys.push(row.get::<String>(0).unwrap());
        }
        keys
    } else {
        remote_db_references_only(database, id, request_id, &secrets).await;
        let (client, connection) = tokio_postgres::connect(database, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            connection.await.unwrap();
        });
        client
            .query(
                "SELECT object_key FROM instance_attachments WHERE instance_id=$1",
                &[&id],
            )
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect()
    };
    for key in keys {
        assert!(
            !waiting_text.contains(&key)
                && !completed_text.contains(&key)
                && !sse.contains(&key)
                && !post_reply_sse.contains(&key),
            "raw key leaked"
        );
    }
    drop(worker);
    drop(server);
}

#[tokio::test]
async fn generated_local_multipart_start_and_reply_survive_restart_with_worker_readable_bytes() {
    let root =
        std::env::temp_dir().join(format!("blkit-attachment-e2e-local-{}", std::process::id()));
    generate(
        &root,
        "attachment_local_fixture",
        "api-worker",
        LOCAL_READER,
        true,
    );
    let database = root.join("instances.db");
    scenario(
        &root,
        database.to_str().unwrap(),
        "attachment_local_fixture",
        "api-worker",
        None,
    )
    .await;
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn generated_combined_and_split_remote_multipart_survive_worker_handoff_via_s3() {
    let (_cloud, endpoint) = seaweed::start().await;
    let postgres = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(postgres.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        postgres.get_host_port_ipv4(5432).await.unwrap()
    );
    for (target, name) in [
        ("api-worker", "attachment_combined_fixture"),
        ("api-worker-split", "attachment_split_fixture"),
    ] {
        let root = std::env::temp_dir().join(format!(
            "blkit-attachment-e2e-{name}-{}",
            std::process::id()
        ));
        generate(&root, name, target, REMOTE_READER, false);
        scenario(&root, &url, name, target, Some(&endpoint)).await;
        fs::remove_dir_all(root).unwrap();
    }
}
