use super::*;
use std::{ffi::OsString, path::PathBuf, process::Command};

const API: &str = "BLKIT_API_DATABASE_URL";
const WORKER: &str = "BLKIT_WORKER_DATABASE_URL";
const ADMIN: &str = "BLKIT_ADMIN_DATABASE_URL";
const API_URL: &str = "postgres://api:sentinel-config-secret@localhost/app";
const WORKER_URL: &str = "postgres://worker:other@localhost/app";
const SECRET: &str = "sentinel-config-secret";

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blkit-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn config(values: &[(&str, &str)]) -> EffectivePostgresConfig {
    EffectivePostgresConfig::from_iter(None, values.iter().copied()).unwrap()
}
fn plaintext() -> PostgresConnectionOptions {
    PostgresConnectionOptions::new(None, Some("true")).unwrap()
}
fn safe_error(error: PostgresConfigError, setting: &str) {
    let diagnostic = format!("{error} {error:?}");
    assert!(diagnostic.contains(setting), "{diagnostic}");
    assert!(!diagnostic.contains(SECRET), "leaked credential");
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn environment_overlays_dotenv_by_name_including_empty_values() {
    let files = Files::new();
    let dotenv = files.write(
        "selected.env",
        format!("export {API}='{WORKER_URL}' # comment\nBLKIT_API_DB_POOL_MAX=9\nEMPTY=old\n"),
    );
    let effective =
        EffectivePostgresConfig::from_iter(Some(&dotenv), [(API, API_URL), ("EMPTY", "")]).unwrap();
    assert_eq!(effective.get(API).unwrap(), Some(API_URL));
    assert_eq!(effective.get("BLKIT_API_DB_POOL_MAX").unwrap(), Some("9"));
    assert_eq!(effective.get("EMPTY").unwrap(), Some(""));
    assert_eq!(effective.get("ABSENT").unwrap(), None);
    assert_eq!(
        effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap()
            .username(),
        "api"
    );
}

#[test]
fn explicitly_selected_dotenv_is_validated_even_when_env_suffices() {
    let files = Files::new();
    for path in [
        files.0.join(SECRET),
        files.0.clone(),
        files.write("syntax.env", format!("{API}={API_URL}\n{SECRET} invalid")),
        files.write("utf8.env", [0xff]),
        files.write("quote.env", format!("{API}='{SECRET}")),
    ] {
        safe_error(
            EffectivePostgresConfig::from_iter(Some(&path), [(API, API_URL)]).unwrap_err(),
            "env-file",
        );
    }
    let empty = files.write("empty.env", "");
    assert!(EffectivePostgresConfig::from_iter(Some(&empty), [(API, API_URL)]).is_ok());
}

#[test]
fn effective_direct_and_file_conflicts_are_rejected_even_if_empty() {
    let files = Files::new();
    let url_file = files.write("url", API_URL);
    let path = url_file.to_str().unwrap();
    for (data, env) in [
        (
            format!("{API}={API_URL}"),
            vec![("BLKIT_API_DATABASE_URL_FILE", path)],
        ),
        (
            format!("BLKIT_API_DATABASE_URL_FILE={path}"),
            vec![(API, API_URL)],
        ),
        (format!("{API}="), vec![("BLKIT_API_DATABASE_URL_FILE", "")]),
        (
            String::new(),
            vec![(API, ""), ("BLKIT_API_DATABASE_URL_FILE", path)],
        ),
    ] {
        let dotenv = files.write("selected.env", data);
        let effective = EffectivePostgresConfig::from_iter(Some(&dotenv), env).unwrap();
        let error = effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap_err();
        assert!(error.to_string().contains("both"));
        safe_error(error, API);
    }
}

#[test]
fn required_roles_have_no_shared_or_administrative_fallback() {
    for values in [
        vec![],
        vec![("DATABASE_URL", API_URL)],
        vec![("BLKIT_DATABASE_URL", API_URL)],
        vec![(ADMIN, API_URL)],
        vec![(API, "")],
        vec![(API, " ")],
    ] {
        safe_error(
            config(&values)
                .resolve_role(DatabaseRole::Api, &plaintext())
                .unwrap_err(),
            API,
        );
    }
    for (role, name) in [(DatabaseRole::Worker, WORKER), (DatabaseRole::Admin, ADMIN)] {
        safe_error(
            config(&[(API, API_URL)])
                .resolve_role(role, &plaintext())
                .unwrap_err(),
            name,
        );
    }
}

#[test]
fn unused_role_settings_are_not_read_or_required() {
    let effective = config(&[
        (API, API_URL),
        (WORKER, ""),
        ("BLKIT_WORKER_DATABASE_URL_FILE", SECRET),
        (ADMIN, SECRET),
        ("BLKIT_ADMIN_DATABASE_URL_FILE", SECRET),
    ]);
    assert!(
        effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .is_ok()
    );
    assert!(
        config(&[(WORKER, WORKER_URL)])
            .resolve_role(DatabaseRole::Worker, &plaintext())
            .is_ok()
    );
    assert!(
        config(&[(ADMIN, "postgres://admin:p@localhost/app")])
            .resolve_role(DatabaseRole::Admin, &plaintext())
            .is_ok()
    );
}

#[test]
fn each_role_uses_only_its_own_secret_file_setting() {
    let files = Files::new();
    let path = files.write("url", API_URL);
    for (role, setting) in [
        (DatabaseRole::Api, "BLKIT_API_DATABASE_URL_FILE"),
        (DatabaseRole::Worker, "BLKIT_WORKER_DATABASE_URL_FILE"),
        (DatabaseRole::Admin, "BLKIT_ADMIN_DATABASE_URL_FILE"),
    ] {
        let database = config(&[(setting, path.to_str().unwrap())])
            .resolve_role(role, &plaintext())
            .unwrap();
        assert_eq!(database.username(), "api");
    }
}

#[test]
fn secret_files_allow_exactly_one_trailing_lf_or_crlf() {
    let files = Files::new();
    for suffix in ["", "\n", "\r\n"] {
        let path = files.write("url", format!("{API_URL}{suffix}"));
        let effective = config(&[("BLKIT_API_DATABASE_URL_FILE", path.to_str().unwrap())]);
        let database = effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap();
        assert_eq!(database.username(), "api");
        assert_eq!(database.password(), Some(SECRET.as_bytes()));
    }
    for contents in [
        "",
        "\n",
        "\r\n",
        " ",
        "\r",
        "not-a-url",
        "postgres://api:p@localhost/app\n\n",
        "postgres://api:p@localhost/app\r\n\r\n",
        "postgres://api:p@localhost/app\npostgres://other:p@localhost/app",
    ] {
        let path = files.write("url", contents);
        safe_error(
            config(&[("BLKIT_API_DATABASE_URL_FILE", path.to_str().unwrap())])
                .resolve_role(DatabaseRole::Api, &plaintext())
                .unwrap_err(),
            "BLKIT_API_DATABASE_URL_FILE",
        );
    }
}

#[test]
fn secret_files_must_be_readable_utf8_and_paths_nonempty() {
    let files = Files::new();
    let invalid = files.write("utf8", [0xff]);
    for path in [
        "",
        files.0.to_str().unwrap(),
        invalid.to_str().unwrap(),
        files.0.join(SECRET).to_str().unwrap(),
    ] {
        safe_error(
            config(&[("BLKIT_API_DATABASE_URL_FILE", path)])
                .resolve_role(DatabaseRole::Api, &plaintext())
                .unwrap_err(),
            "BLKIT_API_DATABASE_URL_FILE",
        );
    }
}

#[cfg(unix)]
#[test]
fn permission_denied_dotenv_and_secret_files_fail_without_exposing_paths() {
    use std::os::unix::fs::PermissionsExt;
    let files = Files::new();
    let path = files.write(SECRET, API_URL);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        eprintln!("permission-denied check skipped: process can bypass file permissions");
        return;
    }
    safe_error(
        EffectivePostgresConfig::from_iter(Some(&path), [(API, API_URL)]).unwrap_err(),
        "env-file",
    );
    safe_error(
        config(&[("BLKIT_API_DATABASE_URL_FILE", path.to_str().unwrap())])
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap_err(),
        "BLKIT_API_DATABASE_URL_FILE",
    );
}

#[test]
fn role_urls_use_driver_decoding_but_reject_missing_identity_or_endpoint() {
    let effective = config(&[(
        API,
        "postgresql://a%70i:p%40ss%3A%2F%3F%23%25%24%26%3D@localhost/a%70p",
    )]);
    let database = effective
        .resolve_role(DatabaseRole::Api, &plaintext())
        .unwrap();
    assert_eq!(database.username(), "api");
    assert_eq!(database.password(), Some(b"p@ss:/?#%$&=".as_slice()));
    assert!(
        database
            .ensure_same_database(
                &config(&[(WORKER, WORKER_URL)])
                    .resolve_role(DatabaseRole::Worker, &plaintext())
                    .unwrap()
            )
            .is_ok()
    );
    for url in [
        "host=localhost user=api dbname=app",
        "postgres://localhost/app",
        "postgres://:p@localhost/app",
        "postgres://api:p@/app",
        "postgres://api:p@localhost/app?dbname=",
        "postgres://api:p@localhost/app\n",
        "postgres://api:p@localhost/app?invalid=sentinel-config-secret",
        "postgres://api:p@localhost/app?port=1,2",
        "postgres://api:p@one,two/app?hostaddr=127.0.0.1",
    ] {
        safe_error(
            config(&[(API, url)])
                .resolve_role(DatabaseRole::Api, &plaintext())
                .unwrap_err(),
            API,
        );
    }
}

#[test]
fn combined_requires_two_distinct_decoded_users() {
    for worker in [API_URL, "postgres://%61pi:other@localhost/app"] {
        safe_error(
            config(&[(API, API_URL), (WORKER, worker)])
                .resolve_combined(&plaintext())
                .unwrap_err(),
            WORKER,
        );
    }
    assert!(
        config(&[(API, API_URL)])
            .resolve_combined(&plaintext())
            .is_err()
    );
    let (api, worker) = config(&[(API, API_URL), (WORKER, WORKER_URL)])
        .resolve_combined(&plaintext())
        .unwrap();
    assert_eq!(api.username(), "api");
    assert_eq!(worker.username(), "worker");
}

#[test]
fn endpoints_accept_harmless_spellings_without_guessing_aliases() {
    for (index, (api, worker)) in [
        (API_URL, "postgresql://worker:p@LOCALHOST:5432/a%70p"),
        (
            "postgres://api:p@[::1]/app",
            "postgres://worker:p@[0:0:0:0:0:0:0:1]:5432/app",
        ),
        (
            "postgres://api:p@localhost",
            "postgres://worker:p@localhost/api",
        ),
        (
            "postgres://api:p@one,two/app",
            "postgres://worker:p@ONE:5432,TWO:5432/app",
        ),
        (
            "postgres://api:p@/app?host=one&host=two&port=1234",
            "postgres://worker:p@one:1234,two:1234/app",
        ),
        (
            "postgres://api:p@localhost/app?hostaddr=127.0.0.1",
            "postgres://worker:p@LOCALHOST/app?hostaddr=127.0.0.1",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = config(&[(API, api), (WORKER, worker)]).resolve_combined(&plaintext());
        assert!(
            result.is_ok(),
            "harmless spelling case {index} rejected: {result:?}"
        );
    }
    for worker in [
        "postgres://worker:p@127.0.0.1/app",
        "postgres://worker:p@localhost:5433/app",
        "postgres://worker:p@localhost/other",
        "postgres://worker:p@localhost/App",
        "postgres://worker:p@localhost/app?hostaddr=127.0.0.1",
        "postgres://worker:p@localhost,other/app",
    ] {
        safe_error(
            config(&[(API, API_URL), (WORKER, worker)])
                .resolve_combined(&plaintext())
                .unwrap_err(),
            WORKER,
        );
    }
    assert!(
        config(&[
            (API, "postgres://api:p@one,two/app"),
            (WORKER, "postgres://worker:p@two,one/app")
        ])
        .resolve_combined(&plaintext())
        .is_err()
    );
}

#[test]
fn admin_endpoint_check_does_not_require_unused_runtime_settings() {
    let admin = config(&[(ADMIN, "postgres://admin:p@localhost/app")])
        .resolve_role(DatabaseRole::Admin, &plaintext())
        .unwrap();
    let api = config(&[(API, API_URL)])
        .resolve_role(DatabaseRole::Api, &plaintext())
        .unwrap();
    assert!(admin.ensure_same_database(&api).is_ok());
    let other = config(&[(WORKER, "postgres://worker:p@elsewhere/app")])
        .resolve_role(DatabaseRole::Worker, &plaintext())
        .unwrap();
    safe_error(admin.ensure_same_database(&other).unwrap_err(), WORKER);
}

#[test]
fn tls_options_use_the_same_overlay_with_strict_values() {
    let files = Files::new();
    let ca = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postgres-tls/ca.pem");
    let dotenv = files.write(
        "selected.env",
        format!(
            "BLKIT_POSTGRES_INSECURE_DEV=true\nBLKIT_POSTGRES_CA_FILE={}\n",
            ca.display()
        ),
    );
    let effective = EffectivePostgresConfig::from_iter(
        Some(&dotenv),
        [("BLKIT_POSTGRES_INSECURE_DEV", "false")],
    )
    .unwrap();
    let options = effective.connection_options().unwrap();
    let role = config(&[(API, API_URL)])
        .resolve_role(DatabaseRole::Api, &options)
        .unwrap();
    assert_eq!(
        role.connection().config.get_ssl_mode(),
        tokio_postgres::config::SslMode::Require
    );
    for value in ["", "TRUE", "False", "1", "0", " true", "true\n", SECRET] {
        safe_error(
            config(&[("BLKIT_POSTGRES_INSECURE_DEV", value)])
                .connection_options()
                .unwrap_err(),
            "BLKIT_POSTGRES_INSECURE_DEV",
        );
    }
    assert!(
        config(&[
            ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
            ("BLKIT_POSTGRES_CA_FILE", ca.to_str().unwrap())
        ])
        .connection_options()
        .is_err()
    );
    for path in ["", SECRET] {
        safe_error(
            config(&[("BLKIT_POSTGRES_CA_FILE", path)])
                .connection_options()
                .unwrap_err(),
            "BLKIT_POSTGRES_CA_FILE",
        );
    }
    for url in [
        "postgres://api:p@localhost/app?sslmode=disable",
        "postgres://api:p@localhost/app?sslmode=prefer",
    ] {
        assert!(
            config(&[(API, url)])
                .resolve_role(DatabaseRole::Api, &options)
                .is_err()
        );
    }
    assert!(
        config(&[(API, "postgres://api:p@localhost/app?sslmode=require")])
            .resolve_role(DatabaseRole::Api, &plaintext())
            .is_err()
    );
}

#[test]
fn configuration_debug_and_error_chains_are_redacted() {
    let url = format!("postgres://{SECRET}:{SECRET}@{SECRET}/{SECRET}?application_name={SECRET}");
    let effective = config(&[(API, &url), ("SECRET", SECRET)]);
    let database = effective
        .resolve_role(DatabaseRole::Api, &plaintext())
        .unwrap();
    assert!(!format!("{effective:?} {database:?} {:?}", database.connection()).contains(SECRET));
    safe_error(
        config(&[(API, SECRET)])
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap_err(),
        API,
    );
}

#[cfg(unix)]
#[test]
fn nonunicode_unused_values_are_ignored_but_required_values_fail_safely() {
    use std::os::unix::ffi::OsStringExt;
    let bad = OsString::from_vec(vec![0xff]);
    let effective = EffectivePostgresConfig::from_iter(
        None,
        [
            (OsString::from(API), OsString::from(API_URL)),
            (OsString::from(WORKER), bad.clone()),
        ],
    )
    .unwrap();
    assert!(
        effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .is_ok()
    );
    safe_error(
        effective
            .resolve_role(DatabaseRole::Worker, &plaintext())
            .unwrap_err(),
        WORKER,
    );
    assert!(
        EffectivePostgresConfig::from_iter(None, [(OsString::from("POOL"), bad)])
            .unwrap()
            .get("POOL")
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn explicit_unix_socket_is_allowed_only_with_existing_plaintext_policy() {
    let effective = config(&[
        (API, "postgres://api:p@%2Ftmp/app"),
        (WORKER, "postgres://worker:p@%2Ftmp/app"),
    ]);
    assert!(effective.resolve_combined(&plaintext()).is_ok());
}

#[test]
fn isolated_environment_configuration_has_no_discovery_execution_or_mutation() {
    if std::env::var_os("BLKIT_CONFIG_CHILD").is_some() {
        let before: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
        let no_file = EffectivePostgresConfig::from_env(None).unwrap();
        assert_eq!(no_file.get(API).unwrap(), Some(API_URL));
        assert_eq!(no_file.get("IMPLICIT").unwrap(), None);
        let effective =
            EffectivePostgresConfig::from_env(Some(Path::new("config/selected.env"))).unwrap();
        assert_eq!(
            effective.get("DOLLARS").unwrap(),
            Some("$HOME ${HOME} $(touch marker) `touch marker`")
        );
        assert_eq!(effective.get("EMPTY").unwrap(), Some(""));
        assert_eq!(effective.get("BLKIT_API_DB_POOL_MAX").unwrap(), Some("7"));
        assert_eq!(
            effective.get("SSL_CERT_FILE").unwrap(),
            Some("missing-native-trust")
        );
        assert!(
            effective
                .resolve_role(DatabaseRole::Worker, &plaintext())
                .is_ok()
        );
        // Relative paths in the file use cwd, not the selected dotenv's parent.
        let options = effective.connection_options().unwrap();
        assert!(effective.resolve_role(DatabaseRole::Api, &options).is_ok());
        let injected = EffectivePostgresConfig::from_iter(
            Some(Path::new("config/selected.env")),
            std::iter::empty::<(&str, &str)>(),
        )
        .unwrap();
        assert_eq!(injected.get("HOME").unwrap(), None);
        assert_eq!(injected.get(API).unwrap(), None);
        let after: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
        assert_eq!(before, after);
        assert!(!Path::new("marker").exists());
        return;
    }
    let files = Files::new();
    std::fs::create_dir(files.0.join("config")).unwrap();
    files.write(".env", "IMPLICIT=should-not-load\n");
    files.write("secret", WORKER_URL);
    let ca = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postgres-tls/ca.pem");
    std::fs::copy(ca, files.0.join("ca.pem")).unwrap();
    files.write("config/selected.env", "DOLLARS=\"$HOME ${HOME} $(touch marker) `touch marker`\"\nEMPTY=\nBLKIT_WORKER_DATABASE_URL_FILE=secret\nBLKIT_API_DB_POOL_MAX=7\nBLKIT_POSTGRES_CA_FILE=ca.pem\nSSL_CERT_FILE=missing-native-trust\n");
    let result = Command::new(std::env::current_exe().unwrap())
        .arg("--exact").arg("postgres_config::tests::isolated_environment_configuration_has_no_discovery_execution_or_mutation")
        .arg("--nocapture").env_clear().env("BLKIT_CONFIG_CHILD", "1").env(API, API_URL).env("HOME", "ambient-sentinel")
        .current_dir(&files.0).output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn resolved_store_rejects_zero_pool_before_network_access() {
    let database = config(&[(API, API_URL)])
        .resolve_role(DatabaseRole::Api, &plaintext())
        .unwrap();
    assert!(
        database
            .connect_store(0)
            .await
            .err()
            .unwrap()
            .contains("positive")
    );
}

#[test]
fn pool_limits_default_and_resolve_only_the_active_role() {
    let empty = config(&[]);
    assert_eq!(empty.api_pool_max().unwrap(), 4);
    assert_eq!(empty.worker_pool_max().unwrap(), 4);
    let api = config(&[
        ("BLKIT_API_DB_POOL_MAX", "2"),
        ("BLKIT_WORKER_DB_POOL_MAX", SECRET),
    ]);
    assert_eq!(api.api_pool_max().unwrap(), 2);
    safe_error(
        api.worker_pool_max().unwrap_err(),
        "BLKIT_WORKER_DB_POOL_MAX",
    );
    let worker = config(&[
        ("BLKIT_API_DB_POOL_MAX", SECRET),
        ("BLKIT_WORKER_DB_POOL_MAX", "6"),
    ]);
    assert_eq!(worker.worker_pool_max().unwrap(), 6);
    safe_error(worker.api_pool_max().unwrap_err(), "BLKIT_API_DB_POOL_MAX");
}

#[test]
fn pool_limits_reject_empty_zero_negative_overflow_and_nonintegers_without_value_echo() {
    for setting in ["BLKIT_API_DB_POOL_MAX", "BLKIT_WORKER_DB_POOL_MAX"] {
        for value in [
            "",
            "0",
            "-1",
            " 2",
            "2 ",
            "1.5",
            "99999999999999999999999999999999",
            SECRET,
        ] {
            let effective = config(&[(setting, value)]);
            let result = if setting == "BLKIT_API_DB_POOL_MAX" {
                effective.api_pool_max()
            } else {
                effective.worker_pool_max()
            };
            safe_error(result.unwrap_err(), setting);
        }
    }
}

#[cfg(unix)]
#[test]
fn pool_limits_reject_non_utf8_only_for_the_selected_role() {
    use std::os::unix::ffi::OsStringExt;
    let effective = EffectivePostgresConfig::from_iter(
        None,
        [(
            OsString::from("BLKIT_API_DB_POOL_MAX"),
            OsString::from_vec(vec![0xff]),
        )],
    )
    .unwrap();
    assert_eq!(effective.worker_pool_max().unwrap(), 4);
    safe_error(
        effective.api_pool_max().unwrap_err(),
        "BLKIT_API_DB_POOL_MAX",
    );
}

#[test]
fn pool_limits_use_the_database_effective_overlay_including_empty_overrides() {
    let files = Files::new();
    let dotenv = files.write(
        "selected.env",
        format!("{API}={API_URL}\nBLKIT_API_DB_POOL_MAX=8\nBLKIT_WORKER_DB_POOL_MAX=9\n"),
    );
    let effective = EffectivePostgresConfig::from_iter(
        Some(&dotenv),
        [(API, WORKER_URL), ("BLKIT_API_DB_POOL_MAX", "2")],
    )
    .unwrap();
    assert_eq!(
        effective
            .resolve_role(DatabaseRole::Api, &plaintext())
            .unwrap()
            .username(),
        "worker"
    );
    assert_eq!(effective.api_pool_max().unwrap(), 2);
    assert_eq!(effective.worker_pool_max().unwrap(), 9);
    let empty =
        EffectivePostgresConfig::from_iter(Some(&dotenv), [("BLKIT_API_DB_POOL_MAX", "")]).unwrap();
    safe_error(empty.api_pool_max().unwrap_err(), "BLKIT_API_DB_POOL_MAX");
}
