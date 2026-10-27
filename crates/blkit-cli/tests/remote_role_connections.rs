//! Live generated entrypoints: catch swapped/shared credentials, listener identity, and pool budgets.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command},
    time::Duration,
};

use tokio::time::{sleep, timeout};

#[path = "../../../tools/test-support/generated_remote.rs"]
mod generated_remote;
#[path = "support/managed_postgres.rs"]
mod managed_postgres;
#[path = "../../../tools/test-support/seaweed.rs"]
mod seaweed;
use generated_remote::Generated;
use managed_postgres::Database;

const PRIVATE: &str = "PRIVATE_ROLE_CONNECTION_SENTINEL";

struct Process {
    child: Child,
    stdout: std::path::PathBuf,
    stderr: std::path::PathBuf,
}

impl Process {
    fn start(binary: &Path, args: &[&str], endpoint: &str, env: &[(&str, &str)]) -> Self {
        let log = std::env::temp_dir().join(format!(
            "blkit-role-connections-{}-{}-{}",
            std::process::id(),
            {
                static NEXT: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            binary.file_name().unwrap().to_str().unwrap()
        ));
        let stdout = log.with_extension("stdout.log");
        let stderr = log.with_extension("stderr.log");
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("TOKIO_WORKER_THREADS", "2")
            .env("BLKIT_POSTGRES_INSECURE_DEV", "true")
            .args(args)
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        seaweed::configure(&mut command, endpoint);
        command.envs(env.iter().copied());
        Self {
            child: command.spawn().unwrap(),
            stdout,
            stderr,
        }
    }

    fn output(&self) -> String {
        let text = format!(
            "{}{}",
            fs::read_to_string(&self.stdout).unwrap(),
            fs::read_to_string(&self.stderr).unwrap()
        );
        for secret in [PRIVATE, "startup-test", "postgres://", "postgresql://"] {
            assert!(
                !text.contains(secret),
                "private process diagnostics withheld"
            );
        }
        text
    }

    fn stop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        self.child.wait().unwrap();
        self.output();
    }

    async fn rejected(&mut self, expected: &str, port: u16) {
        timeout(Duration::from_secs(8), async {
            loop {
                assert!(
                    TcpStream::connect(("127.0.0.1", port)).is_err(),
                    "failed startup served HTTP"
                );
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(!status.success(), "failed startup reported success");
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("failed startup must exit within its connection deadline");
        let text = self.output();
        assert!(text.contains(expected), "expected {expected}; got {text}");
        for success in ["worker ready", "listening", "prepared", "Success"] {
            assert!(!text.contains(success), "false startup success: {text}");
        }
        println!("rejected safely: {expected}; {}", self.stderr.display());
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn request(port: u16, method: &str, path: &str, body: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    stream
}

fn response(mut stream: TcpStream) -> (u16, String) {
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let status = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, text.split_once("\r\n\r\n").unwrap().1.to_owned())
}

fn json(port: u16, method: &str, path: &str, body: &str) -> serde_json::Value {
    let (status, text) = response(request(port, method, path, body));
    assert!([200, 202].contains(&status), "HTTP {status}: {text}");
    serde_json::from_str(&text).unwrap()
}

// Every poll clears the explicit observer transaction's cached pg_stat_activity snapshot.
async fn counts(db: &Database) -> (i64, i64, i64, i64, i64) {
    db.client
        .simple_query("SELECT pg_catalog.pg_stat_clear_snapshot()")
        .await
        .unwrap();
    let row = db.client.query_one(
        "SELECT count(*) FILTER (WHERE usename='blkit_api' AND application_name NOT LIKE 'blkit_%'),
                count(*) FILTER (WHERE usename='blkit_worker'),
                count(*) FILTER (WHERE usename='blkit_api' AND application_name LIKE 'blkit_%'),
                count(*) FILTER (WHERE usename='blkit_worker' AND wait_event_type='Lock' AND (query LIKE 'UPDATE instances SET input=%' OR query LIKE 'UPDATE instances SET lease_until=%')),
                count(*) FILTER (WHERE usename='blkit_api' AND wait_event_type='Lock')
         FROM pg_stat_activity WHERE datname=current_database()", &[]).await.unwrap();
    (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
}

async fn observe(
    db: &Database,
    label: &str,
    api: i64,
    worker: i64,
    condition: impl Fn((i64, i64, i64, i64, i64)) -> bool,
) {
    let mut last = None;
    let seen = timeout(Duration::from_secs(5), async {
        loop {
            let seen = counts(db).await;
            last = Some(seen);
            assert!(
                seen.0 <= api && seen.1 <= worker && seen.2 <= 1,
                "{label}: budget exceeded: {seen:?}"
            );
            if condition(seen) {
                break seen;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("observer deadline: {label}; last counts={last:?}"));
    println!(
        "observer {label}: API pool={}, worker pool={}, API dedicated listener={}, blocked worker checkpoint/renewal writes={}, blocked API operations={}",
        seen.0, seen.1, seen.2, seen.3, seen.4
    );
    for row in db.client.query("SELECT pid, usename, application_name, COALESCE(wait_event_type, ''), left(query, 80) FROM pg_stat_activity WHERE datname=current_database() AND usename IN ('blkit_api', 'blkit_worker') ORDER BY usename, pid", &[]).await.unwrap() {
        println!("  session pid={} user={} app={:?} wait={:?} query={:?}", row.get::<_, i32>(0), row.get::<_, String>(1), row.get::<_, String>(2), row.get::<_, String>(3), row.get::<_, String>(4));
    }
}

async fn ready(process: &mut Process, port: u16) {
    timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "startup exited: {}",
                process.output()
            );
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("generated API readiness deadline");
}

async fn disconnected(db: &Database) {
    observe(db, "all generated sessions closed", 4, 4, |s| {
        s == (0, 0, 0, 0, 0)
    })
    .await;
    db.client
        .batch_execute("DELETE FROM public.workers")
        .await
        .unwrap();
}

async fn combined_budgets(
    db: &Database,
    binary: &Path,
    endpoint: &str,
    api: &str,
    worker: &str,
    limits: Option<(&str, &str)>,
) {
    let port = port();
    let bind = format!("127.0.0.1:{port}");
    let mut env = vec![
        ("BLKIT_API_DATABASE_URL", api),
        ("BLKIT_WORKER_DATABASE_URL", worker),
    ];
    let (api_max, worker_max) = if let Some((a, w)) = limits {
        env.extend([
            ("BLKIT_API_DB_POOL_MAX", a),
            ("BLKIT_WORKER_DB_POOL_MAX", w),
        ]);
        (a.parse::<i64>().unwrap(), w.parse::<i64>().unwrap())
    } else {
        (4, 4)
    };
    let mut process = Process::start(binary, &["12", "60000", &bind], endpoint, &env);
    ready(&mut process, port).await;
    observe(
        db,
        "lazy startup (before subscription)",
        api_max,
        worker_max,
        |s| s == (1, 1, 0, 0, 0),
    )
    .await;

    let anchor = json(
        port,
        "POST",
        "/processes/orders/1/route/instances",
        "{\"value\":\"7\"}",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    timeout(Duration::from_secs(5), async {
        loop {
            if json(port, "GET", &format!("/instances/{anchor}"), "")["status"] == "completed" {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("anchor must complete before worker fault");
    // Interest registration takes FOR SHARE: subscribe before holding unrelated checkpoint rows.
    let mut sse = request(port, "GET", &format!("/instances/{anchor}/events"), "");
    let mut first = String::new();
    while !first.contains("event: snapshot") {
        let mut chunk = [0; 4096];
        let read = sse.read(&mut chunk).unwrap();
        assert!(read > 0, "SSE closed before snapshot");
        first.push_str(std::str::from_utf8(&chunk[..read]).unwrap());
    }
    assert!(first.starts_with("HTTP/1.1 200"));

    // An owned-fixture fault blocks only worker checkpoint writes, never API MVCC reads.
    db.client
        .batch_execute("BEGIN; SELECT pg_advisory_xact_lock(63003)")
        .await
        .unwrap();
    let mut ids = Vec::new();
    for _ in 0..12 {
        ids.push(
            json(
                port,
                "POST",
                "/processes/orders/1/route/instances",
                "{\"value\":\"7\"}",
            )["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    observe(db, "held worker budget", api_max, worker_max, |s| {
        s.1 == worker_max && s.3 == worker_max
    })
    .await;
    let snapshot = json(port, "GET", &format!("/instances/{}", ids[0]), "");
    assert_eq!(snapshot["status"], "running");
    assert!(snapshot.get("checkpoint").is_none());
    observe(
        db,
        "SSE API listener while worker held; API budget independent",
        api_max,
        worker_max,
        |s| s.1 == worker_max && s.2 == 1 && s.3 == worker_max,
    )
    .await;
    assert_eq!(
        json(port, "GET", &format!("/instances/{}", ids[0]), "")["status"],
        "running"
    );
    db.client.batch_execute("COMMIT").await.unwrap();
    timeout(Duration::from_secs(10), async {
        loop {
            let done: i64 = db
                .client
                .query_one(
                    "SELECT count(*) FROM instances WHERE id=ANY($1) AND status='completed'",
                    &[&ids],
                )
                .await
                .unwrap()
                .get(0);
            if done == 12 {
                break;
            }
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "worker exited: {}",
                process.output()
            );
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("released worker checkpoints must finish");
    assert_eq!(
        json(port, "GET", &format!("/instances/{}", ids[0]), "")["result"],
        serde_json::json!("7")
    );
    println!("released worker fault: all 12 instances completed; task limit 12, lease 60000ms");

    // Saturate actual API SELECTs. The dedicated listener is an additional session, not a pool slot.
    db.client
        .batch_execute("BEGIN; LOCK TABLE public.instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let requests: Vec<_> = (0..api_max + 2)
        .map(|_| request(port, "GET", &format!("/instances/{}", ids[0]), ""))
        .collect();
    observe(db, "API maximum + ONE listener", api_max, worker_max, |s| {
        s.0 == api_max && s.2 == 1 && s.4 == api_max
    })
    .await;
    // An extra read must time out instead of exceeding the API maximum; worker sessions cannot substitute.
    let (status, _) = response(request(port, "GET", &format!("/instances/{}", ids[0]), ""));
    assert_eq!(status, 500);
    println!(
        "excess API checkout returned HTTP 500 with all {api_max} pool sessions held; no overflow"
    );
    let held = counts(db).await;
    assert_eq!((held.0, held.2), (api_max, 1));
    db.client.batch_execute("COMMIT").await.unwrap();
    for stream in requests {
        let (status, _) = response(stream);
        assert!([200, 500].contains(&status));
    }
    drop(sse);
    process.stop();
    println!(
        "budget proof passed: API {api_max}+1 listener / worker {worker_max}; {}",
        process.stdout.display()
    );
    disconnected(db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_roles_authenticate_separately_fail_closed_and_enforce_independent_budgets() {
    let db = Database::start().await;
    let settings = db.settings();
    let api = settings.get("BLKIT_API_DATABASE_URL").unwrap().unwrap();
    let worker = settings.get("BLKIT_WORKER_DATABASE_URL").unwrap().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .env_clear()
        .args(["db", "init"])
        .env("BLKIT_ADMIN_DATABASE_URL", &db.url)
        .env("BLKIT_API_DATABASE_URL", api)
        .env("BLKIT_WORKER_DATABASE_URL", worker)
        .env("BLKIT_POSTGRES_INSECURE_DEV", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI fixture preparation failed (private output withheld)"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Database initialized."));
    // Install only in this disposable fixture, after real CLI preparation; canonical assets stay untouched.
    db.client.batch_execute("CREATE FUNCTION public.hold_worker_checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF current_user='blkit_worker' AND OLD.checkpoint IS NULL THEN PERFORM pg_catalog.pg_advisory_xact_lock(63003); END IF; RETURN NEW; END $$; CREATE TRIGGER hold_worker_checkpoint BEFORE UPDATE OF checkpoint ON public.instances FOR EACH ROW EXECUTE FUNCTION public.hold_worker_checkpoint()").await.unwrap();
    let (_cloud, endpoint) = seaweed::start().await;
    let combined = Generated::new("api-worker");
    let binary = combined
        .build(false)
        .join(format!("{}-api-worker", combined.name));

    // Existing correct wiring is a baseline, not a fabricated RED implementation cycle.
    combined_budgets(&db, &binary, &endpoint, api, worker, Some(("2", "3"))).await;
    combined_budgets(&db, &binary, &endpoint, api, worker, None).await;

    let sentinel = TcpListener::bind("127.0.0.1:0").unwrap();
    sentinel.set_nonblocking(true).unwrap();
    let api_probe = format!(
        "postgres://blkit_api:{PRIVATE}@{}/postgres",
        sentinel.local_addr().unwrap()
    );
    let worker_probe = api_probe.replace("blkit_api", "blkit_worker");
    let same_login = api_probe.replace(PRIVATE, &format!("{PRIVATE}_OTHER_PASSWORD"));
    let other_db = worker_probe.replace("/postgres", "/other");
    let other_sentinel = TcpListener::bind("127.0.0.1:0").unwrap();
    other_sentinel.set_nonblocking(true).unwrap();
    let other_endpoint = format!(
        "postgres://blkit_worker:{PRIVATE}@{}/postgres",
        other_sentinel.local_addr().unwrap()
    );
    for (a, w, expected) in [
        (None, Some(worker_probe.as_str()), "BLKIT_API_DATABASE_URL"),
        (Some(api_probe.as_str()), None, "BLKIT_WORKER_DATABASE_URL"),
        (
            Some(api_probe.as_str()),
            Some(same_login.as_str()),
            "different login user",
        ),
        (Some(api_probe.as_str()), Some(other_db.as_str()), "same"),
        (
            Some(api_probe.as_str()),
            Some(other_endpoint.as_str()),
            "same",
        ),
    ] {
        let port = port();
        let bind = format!("127.0.0.1:{port}");
        let mut env = vec![
            ("BLKIT_ADMIN_DATABASE_URL", api_probe.as_str()),
            ("DATABASE_URL", api_probe.as_str()),
        ];
        if let Some(a) = a {
            env.push(("BLKIT_API_DATABASE_URL", a));
        }
        if let Some(w) = w {
            env.push(("BLKIT_WORKER_DATABASE_URL", w));
        }
        let mut process = Process::start(&binary, &["12", "60000", &bind], &endpoint, &env);
        process.rejected(expected, port).await;
        assert!(
            matches!(sentinel.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "invalid combined configuration attempted a connection/fallback"
        );
        assert!(
            matches!(other_sentinel.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "invalid combined configuration contacted the different endpoint"
        );
    }
    for (a, w) in [
        (api.replace("startup-test", PRIVATE), worker.to_owned()),
        (api.to_owned(), worker.replace("startup-test", PRIVATE)),
    ] {
        let port = port();
        let bind = format!("127.0.0.1:{port}");
        let mut process = Process::start(
            &binary,
            &["12", "60000", &bind],
            &endpoint,
            &[
                ("BLKIT_API_DATABASE_URL", &a),
                ("BLKIT_WORKER_DATABASE_URL", &w),
            ],
        );
        process.rejected("database operation failed", port).await;
        assert_eq!(
            db.client
                .query_one("SELECT count(*) FROM workers", &[])
                .await
                .unwrap()
                .get::<_, i64>(0),
            0,
            "bad active password advertised a worker"
        );
        disconnected(&db).await;
    }

    // Every actual single-role output ignores invalid unused roles/admin/generic settings.
    for target in ["api-only", "worker-only", "worker", "api-worker-split"] {
        let project = Generated::new(target);
        for api_member in if target == "api-worker-split" {
            vec![false, true]
        } else {
            vec![false]
        } {
            let api_role = target == "api-only" || api_member;
            let mode = if api_role { "api" } else { "worker" };
            let binary = project
                .build(api_member)
                .join(format!("{}-{mode}", project.name));
            let port = port();
            let bind = format!("127.0.0.1:{port}");
            let args: Vec<&str> = if api_role {
                vec![&bind]
            } else {
                vec!["12", "60000"]
            };
            let (active, inactive, unused_pool, url) = if api_role {
                (
                    "BLKIT_API_DATABASE_URL",
                    "BLKIT_WORKER_DATABASE_URL",
                    "BLKIT_WORKER_DB_POOL_MAX",
                    api,
                )
            } else {
                (
                    "BLKIT_WORKER_DATABASE_URL",
                    "BLKIT_API_DATABASE_URL",
                    "BLKIT_API_DB_POOL_MAX",
                    worker,
                )
            };
            let mut env = vec![
                (active, url),
                (inactive, PRIVATE),
                (unused_pool, PRIVATE),
                ("BLKIT_ADMIN_DATABASE_URL", PRIVATE),
                ("DATABASE_URL", PRIVATE),
                ("BLKIT_ADMIN_DATABASE_URL_FILE", PRIVATE),
            ];
            let mut process = Process::start(&binary, &args, &endpoint, &env);
            if api_role {
                ready(&mut process, port).await;
                let id: String = db
                    .client
                    .query_one("SELECT id FROM instances LIMIT 1", &[])
                    .await
                    .unwrap()
                    .get(0);
                assert_eq!(
                    json(port, "GET", &format!("/instances/{id}"), "")["status"],
                    "completed"
                );
                observe(&db, "single-role API only", 4, 0, |s| {
                    (1..=4).contains(&s.0) && (s.1, s.2, s.3, s.4) == (0, 0, 0, 0)
                })
                .await;
            } else {
                timeout(Duration::from_secs(5), async {
                    loop {
                        assert!(
                            process.child.try_wait().unwrap().is_none(),
                            "single worker exited: {}",
                            process.output()
                        );
                        let n: i64 = db
                            .client
                            .query_one(
                                "SELECT count(*) FROM workers WHERE id=$1",
                                &[&format!("worker-{}", process.child.id())],
                            )
                            .await
                            .unwrap()
                            .get(0);
                        if n == 1 && process.output().contains("worker ready") {
                            break;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("single-role worker advertisement deadline");
                observe(&db, "single-role worker only", 0, 4, |s| {
                    s == (0, 1, 0, 0, 0)
                })
                .await;
                assert!(process.output().contains("worker ready"));
            }
            process.stop();
            disconnected(&db).await;
            let wrong_password = url.replace("startup-test", PRIVATE);
            env[0].1 = &wrong_password;
            let mut process = Process::start(&binary, &args, &endpoint, &env);
            process.rejected("database operation failed", port).await;
            assert_eq!(
                db.client
                    .query_one("SELECT count(*) FROM workers", &[])
                    .await
                    .unwrap()
                    .get::<_, i64>(0),
                0
            );
            disconnected(&db).await;
            println!("live single-role proof: {target}/{mode}");
        }
    }
}
