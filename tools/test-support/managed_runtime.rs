//! Actual CLI provisioning for disposable runtime-child fixtures, not fake role metadata.
use blkit_core::postgres_config::EffectivePostgresConfig;
use std::process::Command;

pub fn role_url(admin_url: &str, role: &str) -> String {
    assert!(["api", "worker"].contains(&role));
    let endpoint = admin_url.rsplit_once('@').unwrap().1;
    format!("postgres://blkit_{role}:startup-test@{endpoint}")
}

pub async fn prepare(admin_url: &str) {
    let settings = EffectivePostgresConfig::from_iter(
        None,
        [
            ("BLKIT_ADMIN_DATABASE_URL", admin_url.to_owned()),
            ("BLKIT_API_DATABASE_URL", role_url(admin_url, "api")),
            ("BLKIT_WORKER_DATABASE_URL", role_url(admin_url, "worker")),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        blkit_cli::postgres_admin::prepare_database(
            &settings,
            blkit_cli::postgres_admin::Preparation::Init { role_prefix: None },
        )
        .await
        .unwrap(),
        blkit_cli::postgres_admin::PreparationOutcome::Initialized
    );
}

/// Runtime children receive only their active role(s); admin stays in the parent fixture.
#[allow(dead_code)]
pub fn configure(command: &mut Command, admin_url: &str, roles: &[&str]) {
    for name in [
        "BLKIT_DATABASE_URL",
        "BLKIT_DATABASE_URL_FILE",
        "BLKIT_ADMIN_DATABASE_URL",
        "BLKIT_ADMIN_DATABASE_URL_FILE",
        "BLKIT_API_DATABASE_URL",
        "BLKIT_API_DATABASE_URL_FILE",
        "BLKIT_WORKER_DATABASE_URL",
        "BLKIT_WORKER_DATABASE_URL_FILE",
        "BLKIT_POSTGRES_CA_FILE",
        "BLKIT_API_DB_POOL_MAX",
        "BLKIT_WORKER_DB_POOL_MAX",
    ] {
        command.env_remove(name);
    }
    command
        .env("BLKIT_POSTGRES_INSECURE_DEV", "true")
        .env("TOKIO_WORKER_THREADS", "2");
    for role in roles {
        let setting = match *role {
            "api" => "BLKIT_API_DATABASE_URL",
            "worker" => "BLKIT_WORKER_DATABASE_URL",
            _ => panic!("invalid fixture role"),
        };
        command.env(setting, role_url(admin_url, role));
    }
    assert!(
        command
            .get_args()
            .all(|arg| !arg.to_string_lossy().contains("://")),
        "credential-bearing child argv"
    );
}

/// Inspect real live runtime sessions, not preparation metadata.
#[allow(dead_code)]
pub async fn assert_sessions(admin_url: &str, roles: &[&str]) {
    let (client, driver) = tokio_postgres::connect(admin_url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::pin!(driver);
    tokio::select! {
        _ = async {
            for role in roles {
                let user = format!("blkit_{role}");
                let count: i64 = client.query_one("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND usename=$1", &[&user]).await.unwrap().get(0);
                assert!(count > 0, "no live managed {role} session");
                println!("live managed identity {user}: {count} session(s)");
            }
        } => {},
        _ = &mut driver => panic!("runtime identity observer stopped"),
    }
}

#[allow(dead_code)]
pub fn assert_child_configuration(child: &std::process::Child, roles: &[&str]) {
    #[cfg(target_os = "linux")]
    {
        let args = std::fs::read(format!("/proc/{}/cmdline", child.id())).unwrap();
        for secret in [
            "postgres://",
            "postgresql://",
            "startup-test",
            "postgres:postgres",
        ] {
            assert!(
                !String::from_utf8_lossy(&args).contains(secret),
                "credential-bearing child argv"
            );
        }
        let environment = std::fs::read(format!("/proc/{}/environ", child.id())).unwrap();
        let names: Vec<_> = environment
            .split(|byte| *byte == 0)
            .filter_map(|entry| entry.split(|byte| *byte == b'=').next())
            .collect();
        for name in [
            "BLKIT_ADMIN_DATABASE_URL",
            "BLKIT_ADMIN_DATABASE_URL_FILE",
            "BLKIT_DATABASE_URL",
            "BLKIT_DATABASE_URL_FILE",
        ] {
            assert!(
                !names.contains(&name.as_bytes()),
                "runtime inherited administrative/generic credentials"
            );
        }
        for role in ["api", "worker"] {
            let direct = format!("BLKIT_{}_DATABASE_URL", role.to_uppercase());
            let file = format!("{direct}_FILE");
            if !roles.contains(&role) {
                assert!(
                    !names.contains(&direct.as_bytes()) && !names.contains(&file.as_bytes()),
                    "runtime inherited inactive role credentials"
                );
            }
        }
        println!(
            "child {}: credential-free argv and isolated runtime environment",
            child.id()
        );
    }
}
