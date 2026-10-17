use blkit_transpiler as blkit;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn cargo_build(root: &std::path::Path) -> Result<(), String> {
    let manifest = root.join(".blkit/Cargo.toml");
    let mut command = std::process::Command::new("cargo");
    command.args(["build", "--manifest-path"]).arg(manifest);
    if root.join(".blkit/Cargo.lock").exists() {
        command.arg("--locked");
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo build failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn project(manifest: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "blkit-project-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("blkit.toml"), manifest).unwrap();
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    root
}

const SOURCE: &str = "namespace orders;\nversion \"1\";\n";
const MANIFEST: &str =
    "[project]\nname = \"orders\"\nblkit = \"0.1.0\"\nbuild_target = \"crate\"\n";

#[test]
fn generated_project_depends_on_version_matched_core_not_transpiler() {
    let source = "namespace orders; version \"1\"; start_event start { output value: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.value -> done.result; }";
    let root = project(MANIFEST, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    let manifest = fs::read_to_string(root.join(".blkit/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("blkit-core = { version = \"=0.1.0\""),
        "{manifest}"
    );
    assert!(!manifest.contains("blkit-transpiler"), "{manifest}");
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../blkit-core")
        .canonicalize()
        .unwrap();
    assert!(
        manifest.contains(&format!("path = {:?}", core.to_str().unwrap())),
        "generated project must use the checkout's core: {manifest}"
    );
    assert!(!manifest.contains("\nblkit ="), "{manifest}");
    let generated = fs::read_to_string(root.join(".blkit/src/lib.rs")).unwrap();
    assert!(
        generated.contains("blkit_core::compiled_graph"),
        "{generated}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn numeric_project_builds_checked_arithmetic() {
    let source = "namespace orders; version \"1\"; start_event start { output value: Number; } end_event done { input result: Number; } decision_task calculate { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression (value + 0.2) / value; } } process route { flow start -> calculate; flow calculate -> done; bind start.value -> calculate.value; bind calculate.result -> done.result; }";
    let root = project(MANIFEST, &[("numbers.bl", source)]);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["transpile"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    cargo_build(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_list_decision_compiles_and_propagates_checked_errors() {
    let declarations = r#"namespace orders; version "1";
start_event start { output values: List<Number>; output position: Number; }
end_event done { input result: Number; }
decision_task calculate {
  input values: List<Number>;
  input position: Number;
  output result: Number = total;
  knowledge positives {
    input items: List<Number>;
    output result: List<Number>;
    expression items[.item > 0];
  }
  context total {
    output result: Number;
    entry selected: List<Number> = positives(values);
    result sum(distinct(selected)) + values[position];
  }
}"#;
    let route = r#"namespace orders; version "1";
process route {
  flow start -> calculate;
  flow calculate -> done;
  bind start.values -> calculate.values;
  bind start.position -> calculate.position;
  bind calculate.result -> done.result;
}"#;
    let root = project(MANIFEST, &[("a.bl", route), ("z.bl", declarations)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    let manifest = root.join(".blkit/Cargo.toml");
    let lib = root.join(".blkit/src/lib.rs");
    let mut source = fs::read_to_string(&lib).unwrap();
    source.push_str(r#"
#[cfg(test)] mod list_checks {
    use super::*;
    #[test] fn generated_project_list_behavior() {
        let n = |value: &str| value.parse::<scope_0::Number>().unwrap();
        assert_eq!(scope_0::calculate(vec![n("3"), n("-1"), n("3")], n("2")).unwrap(), n("2"));
        for position in ["0", "4"] {
            let error = scope_0::calculate(vec![n("3")], n(position)).unwrap_err();
            assert!(error.to_string().contains("list position"), "{error}");
        }
        let graph = named_graph_definitions().remove(0);
        let input = serde_json::json!({"values":["3","-1","3"], "position":"2"});
        let mut state = graph.checkpoint(&input).unwrap();
        assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("2"));
        let invalid = serde_json::json!({"values":[], "position":"1"});
        let error = graph.checkpoint(&invalid).and_then(|mut state| graph.run(&invalid, &mut state)).unwrap_err();
        assert!(error.contains("list position"), "{error}");
    }
}
"#);
    fs::write(lib, source).unwrap();
    let result = std::process::Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_project_file_unknown_type_uses_its_own_utf8_byte_and_emits_no_rust() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; }";
    let second =
        "namespace orders; version \"1\";\n# café\nend_event done { input amount: Missing; }";
    let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap_err();
    assert!(error.contains("z.bl: unknown type: Missing"), "{error}");
    assert!(
        error.contains(&format!("at byte {}", second.find("Missing").unwrap())),
        "{error}"
    );
    assert!(!error.contains("a.bl: unknown"), "{error}");
    assert!(!root.join(".blkit/src/lib.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_project_file_invalid_binding_uses_its_own_byte_and_emits_no_rust() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; }";
    let second = "namespace orders; version \"1\"; process route { flow start -> done; bind start.missing -> done.result; }";
    let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap_err();
    assert!(
        error.contains("z.bl: unknown binding output: start.missing"),
        "{error}"
    );
    assert!(
        error.contains(&format!(
            "at byte {}",
            second.find("start.missing").unwrap()
        )),
        "{error}"
    );
    assert!(!error.contains("a.bl: unknown"), "{error}");
    assert!(!root.join(".blkit/src/lib.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_project_file_binding_outside_process_locates_its_node_without_rust() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; }";
    let second = "namespace orders; version \"1\";\n# café\nprocess route { flow start -> done; bind start.amount -> spare.result; }";
    let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap_err();
    assert!(
        error.contains("z.bl: binding node outside process: start -> spare"),
        "{error}"
    );
    assert!(
        error.contains(&format!("at byte {}", second.find("spare.result").unwrap())),
        "{error}"
    );
    assert!(!error.contains("a.bl: binding"), "{error}");
    assert!(!root.join(".blkit/src/lib.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_project_file_binding_outside_process_locates_its_source_node_without_rust() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; }";
    let second = "namespace orders; version \"1\";\n# café\nprocess route { flow start -> done; bind spare.result -> done.result; }";
    let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap_err();
    assert!(
        error.contains("z.bl: binding node outside process: spare -> done"),
        "{error}"
    );
    assert!(
        error.contains(&format!("at byte {}", second.find("spare.result").unwrap())),
        "{error}"
    );
    assert!(!error.contains("a.bl: binding"), "{error}");
    assert!(!root.join(".blkit/src/lib.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unlocatable_project_error_keeps_both_paths_without_a_fake_byte() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; }";
    let second = "namespace orders; version \"1\"; process route { }";
    let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap_err();
    assert!(error.contains("a.bl") && error.contains("z.bl"), "{error}");
    assert!(error.contains("start"), "{error}");
    assert!(!error.contains("at byte"), "{error}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cli_project_transpile_labels_second_file_and_keeps_global_error_unlocated() {
    let first = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; }";
    for (second, detail, excerpt, located) in [
        (
            "namespace orders; version \"1\";\n# café\nprocess route { retry max_retries 1 retry_for \"bogus\" retry_delay \"1s\" backoff exponential; }",
            "invalid retry duration: bogus",
            "retry_for \"bogus\"",
            Some("label at line 3, columns 47 to 53"),
        ),
        (
            "namespace orders; version \"1\";\n# café\nprocess route { flow start -> done; bind start.missing -> done.result; }",
            "unknown binding output: start.missing",
            "bind start.missing -> done.result;",
            Some("label at line 3, columns 42 to 54"),
        ),
        (
            "namespace orders; version \"1\";\n# café\n@",
            "invalid character",
            "snippet line 3: @",
            Some("label at line 3, column 1"),
        ),
        (
            "namespace orders; version \"1\"; process route { }",
            "start",
            "snippet line",
            None,
        ),
    ] {
        let root = project(MANIFEST, &[("a.bl", first), ("z.bl", second)]);
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("transpile")
            .arg(&root)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(
            stderr.contains(detail) && stderr.contains(&*root.join("z.bl").to_string_lossy()),
            "{stderr}"
        );
        if let Some(label) = located {
            assert!(
                stderr.contains(excerpt) && stderr.contains(label),
                "{stderr}"
            );
            assert!(
                !stderr.contains(&format!(
                    "Begin snippet for {}",
                    root.join("a.bl").display()
                )),
                "{stderr}"
            );
        } else {
            assert!(
                stderr.contains(&*root.join("a.bl").to_string_lossy()),
                "{stderr}"
            );
            assert!(
                !stderr.contains("Begin snippet") && !stderr.contains("label at"),
                "{stderr}"
            );
        }
        assert!(
            !stderr.contains('\u{1b}') && !stderr.contains('\r'),
            "{stderr:?}"
        );
        assert!(!root.join(".blkit/src/lib.rs").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cli_project_source_read_error_has_no_empty_source_prefix() {
    let root = project(
        MANIFEST,
        &[("broken.bl", "namespace orders; version \"1\";")],
    );
    fs::write(root.join("broken.bl"), [0xff]).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("transpile")
        .arg(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains("broken.bl"), "{stderr}");
    assert!(stderr.contains("UTF-8"), "{stderr}");
    assert!(!stderr.contains(": : "), "{stderr}");
    assert!(
        !stderr
            .lines()
            .any(|line| line.trim_start().starts_with(": ")),
        "{stderr}"
    );
    assert!(!root.join(".blkit/src/lib.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn named_dictionary_resolves_across_project_files() {
    let root = project(
        MANIFEST,
        &[
            (
                "a.bl",
                "namespace orders; version \"1\"; start_event start { output input: Order; }",
            ),
            (
                "z.bl",
                "namespace orders; version \"1\"; Order = {total: Number};",
            ),
        ],
    );
    let groups = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap();
    assert_eq!(groups[0].records[0].name, "Order");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn braced_project_merges_forward_peer_declarations_without_cargo() {
    let route = "namespace orders;\nversion \"1\";\nprocess route { flow start -> decide; flow decide -> done; bind start.amount -> decide.amount; bind decide.result -> done.result; }\n";
    let peers = "namespace orders;\nversion \"1\";\nstart_event start { output amount: Number; }\nend_event done { input result: Number; }\ndecision_task decide { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }\n";
    let root = project(MANIFEST, &[("a.bl", route), ("z.bl", peers)]);
    let program = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap();
    assert_eq!(program[0].peer_nodes.len(), 2);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["transpile"])
        .arg(&root)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lib = fs::read_to_string(root.join(".blkit/src/lib.rs")).unwrap();
    assert!(lib.contains("scope_0::named_graph_definitions()"));
    cargo_build(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn braced_project_rejects_duplicate_and_out_of_scope_peers_without_cargo() {
    let peers = "namespace orders; version \"1\"; start_event start { output amount: Number; }";
    let root = project(MANIFEST, &[("a.bl", peers), ("z.bl", peers)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap_err();
    assert!(
        error.contains("start") && error.contains("a.bl") && error.contains("z.bl"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
    let wrong_scope = "namespace other; version \"1\"; end_event done { input result: Number; } process route { flow start -> done; bind start.amount -> done.result; }";
    let root = project(MANIFEST, &[("orders.bl", peers), ("other.bl", wrong_scope)]);
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap_err();
    assert!(
        error.contains("start") && error.contains("other.bl"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn braced_project_resolves_forward_subprocess_and_separates_versions() {
    let parent = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } subprocess called { process child; input amount: Number; output result: Number; } decision_task echo { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } } process parent { flow start -> called; flow called -> done; bind start.amount -> called.amount; bind called.result -> done.result; }";
    let child = "namespace orders; version \"1\"; process child { flow start -> echo; flow echo -> done; bind start.amount -> echo.amount; bind echo.result -> done.result; }";
    let root = project(MANIFEST, &[("a.bl", parent), ("z.bl", child)]);
    let groups = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap();
    assert_eq!(groups[0].processes.len(), 2);
    fs::remove_dir_all(root).unwrap();
    let version_two =
        "namespace orders; version \"2\"; start_event start { output amount: Number; }";
    let root = project(
        MANIFEST,
        &[
            ("one.bl", parent),
            ("child.bl", child),
            ("two.bl", version_two),
        ],
    );
    assert_eq!(
        blkit::project::Project::load(&root)
            .unwrap()
            .programs()
            .unwrap()
            .len(),
        2
    );
    fs::write(root.join("two.bl"), "namespace orders; version \"2\"; process other { flow start -> done; bind start.amount -> done.result; }").unwrap();
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap_err();
    assert!(
        error.contains("start") && error.contains("two.bl"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn braced_project_rejects_legacy_extension_calls_before_cargo_resolution() {
    let old = "namespace orders; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } process route { node paid = task payments.charge(start.amount); link start -> paid; link paid -> done; }";
    let manifest =
        format!("{MANIFEST}[dependencies]\npayments = {{ version = \"1\", path = \"missing\" }}\n");
    let root = project(&manifest, &[("route.bl", old)]);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["transpile"])
        .arg(&root)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid process statement"));
    assert!(!root.join(".blkit/Cargo.toml").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transpilation_generates_each_target_without_compiling() {
    for target in ["crate", "worker", "server"] {
        let root = project(
            &MANIFEST.replace("\"crate\"", &format!("\"{target}\"")),
            &[("source.bl", SOURCE)],
        );
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("transpile")
            .arg(&root)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(root.join(".blkit/Cargo.toml").exists());
        assert!(root.join(".blkit/src/lib.rs").exists());
        if target != "crate" {
            assert!(
                root.join(format!(".blkit/src/bin/orders-{target}.rs"))
                    .exists()
            );
        }
        assert!(!root.join(".blkit/target").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn retargeting_combined_project_to_api_only_removes_stale_binary() {
    let root = project(
        &MANIFEST.replace(
            "build_target = \"crate\"",
            "build_target = \"api-worker\"\npersistence = \"local\"",
        ),
        &[("route.bl", SOURCE)],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-api-worker.rs").exists());
    fs::write(
        root.join("blkit.toml"),
        MANIFEST.replace("crate", "api-only"),
    )
    .unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(!root.join(".blkit/src/bin/orders-api-worker.rs").exists());
    assert!(!root.join(".blkit/src/lib.rs").exists());
    cargo_build(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_free_api_only_builds_without_generated_definitions() {
    let root = project(&MANIFEST.replace("crate", "api-only"), &[]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(!root.join(".blkit/src/lib.rs").exists());
    assert!(!root.join(".blkit/src/scope_0.rs").exists());
    let manifest = fs::read_to_string(root.join(".blkit/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("api-server")
            && manifest.contains("remote-persistence")
            && !manifest.contains("worker\""),
        "{manifest}"
    );
    cargo_build(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
    let with_source = project(
        &MANIFEST.replace("crate", "api-only"),
        &[("route.bl", SOURCE)],
    );
    blkit::project::Project::load(&with_source)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(!with_source.join(".blkit/src/lib.rs").exists());
    assert!(!with_source.join(".blkit/src/scope_0.rs").exists());
    fs::remove_dir_all(with_source).unwrap();
}

#[tokio::test]
async fn generated_remote_roles_validate_only_active_pool_limits() {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

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
    for (target, persistence, binary, active, inactive) in [
        (
            "api-only",
            "",
            "orders-api",
            "BLKIT_API_DB_POOL_MAX",
            "BLKIT_WORKER_DB_POOL_MAX",
        ),
        (
            "worker-only",
            "",
            "orders-worker",
            "BLKIT_WORKER_DB_POOL_MAX",
            "BLKIT_API_DB_POOL_MAX",
        ),
        (
            "api-worker-split",
            "",
            "orders-api",
            "BLKIT_API_DB_POOL_MAX",
            "BLKIT_WORKER_DB_POOL_MAX",
        ),
        (
            "api-worker-split",
            "",
            "orders-worker",
            "BLKIT_WORKER_DB_POOL_MAX",
            "BLKIT_API_DB_POOL_MAX",
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            "BLKIT_API_DB_POOL_MAX",
            "BLKIT_WORKER_DB_POOL_MAX",
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            "BLKIT_WORKER_DB_POOL_MAX",
            "BLKIT_API_DB_POOL_MAX",
        ),
    ] {
        let root = project(
            &MANIFEST.replace(
                "build_target = \"crate\"\n",
                &format!("build_target = {target:?}\n{persistence}"),
            ),
            &[("route.bl", SOURCE)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        cargo_build(&root).unwrap();
        let binary = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or(root.join(".blkit/target"))
            .join("debug")
            .join(binary);
        let mut args = vec![url.as_str()];
        if binary.to_string_lossy().ends_with("orders-worker") {
            args.extend(["2", "1000"]);
        }
        if binary.to_string_lossy().ends_with("orders-api-worker") {
            args.extend(["2", "1000", "127.0.0.1:0"]);
        }
        if binary.to_string_lossy().ends_with("orders-api") {
            args.push("127.0.0.1:0");
        }
        for invalid in ["0", "malformed"] {
            let diagnostic_path = root.join("invalid-pool-setting.log");
            let diagnostic_file = fs::File::create(&diagnostic_path).unwrap();
            let mut child = Child(
                Command::new(&binary)
                    .args(&args)
                    .env(active, invalid)
                    .env_remove(inactive)
                    .env_remove("BLKIT_LOG_OUTPUTS")
                    .stdout(Stdio::from(diagnostic_file.try_clone().unwrap()))
                    .stderr(Stdio::from(diagnostic_file))
                    .spawn()
                    .unwrap(),
            );
            let exit = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Some(exit) = child.0.try_wait().unwrap() {
                        break exit;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            let diagnostic = fs::read_to_string(&diagnostic_path).unwrap();
            assert!(
                exit.is_ok_and(|status| !status.success()) && diagnostic.contains(active),
                "{target} {active}={invalid}: did not reject active setting within 3s: {diagnostic}"
            );
        }
        if target != "api-worker" {
            let mut child = Child(
                Command::new(&binary)
                    .args(&args)
                    .env_remove(active)
                    .env(inactive, "malformed")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "{target}: ignored {inactive} prevented startup"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn generated_remote_role_pool_budgets_are_bounded_independently() {
    use blkit_core::{postgres_store::PostgresStore, runtime::Instance};
    use std::{
        io::Write,
        process::{Command, Stdio},
        time::Duration,
    };
    use testcontainers_modules::testcontainers::core::ExecCommand;

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let postgres = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(postgres.get_host().await.unwrap().to_string());
    let base_url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        postgres.get_host_port_ipv4(5432).await.unwrap()
    );
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } } process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    for (target, persistence, binary, api_max, worker_max, expected_max) in [
        ("api-only", "", "orders-api", None, None, 4),
        ("api-only", "", "orders-api", Some("1"), Some("bad"), 1),
        ("api-only", "", "orders-api", Some("2"), Some("bad"), 2),
        ("worker-only", "", "orders-worker", None, None, 4),
        (
            "worker-only",
            "",
            "orders-worker",
            Some("bad"),
            Some("2"),
            2,
        ),
        (
            "worker-only",
            "",
            "orders-worker",
            Some("bad"),
            Some("1"),
            1,
        ),
        (
            "api-worker-split",
            "",
            "orders-api",
            Some("1"),
            Some("bad"),
            1,
        ),
        (
            "api-worker-split",
            "",
            "orders-worker",
            Some("bad"),
            Some("1"),
            1,
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            None,
            None,
            8,
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            Some("1"),
            Some("1"),
            2,
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            Some("1"),
            Some("2"),
            3,
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
            Some("2"),
            Some("1"),
            3,
        ),
    ] {
        let root = project(
            &MANIFEST.replace(
                "build_target = \"crate\"\n",
                &format!("build_target = {target:?}\n{persistence}"),
            ),
            &[("route.bl", source)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        let worker_active = binary != "orders-api";
        let app = format!("pool_{}", NEXT.fetch_add(1, Ordering::Relaxed));
        if worker_active {
            let lib = root.join(".blkit/src/lib.rs");
            let generated = fs::read_to_string(&lib).unwrap();
            fs::write(&lib, format!("{}\n{}", generated.replacen("pub fn named_graph_definitions()", "fn compiled_graph_definitions()", 1), r#"
pub fn named_graph_definitions() -> Vec<blkit_core::compiled_graph::GraphDefinition> {
    let mut graphs = compiled_graph_definitions();
    let task = graphs[0].nodes.iter_mut().find(|node| node.name == "echo").unwrap();
    task.kind = blkit_core::compiled_graph::GraphNodeKind::Task(std::sync::Arc::new(|input, _| {
        let gate = std::path::PathBuf::from(std::env::var("BLKIT_TEST_GATE_DIR").unwrap());
        std::fs::write(gate.join(format!("entered-{}", input["input"].as_str().unwrap())), "").unwrap();
        while !gate.join("release").exists() { std::thread::sleep(std::time::Duration::from_millis(10)); }
        Ok(input["input"].clone())
    }));
    graphs
}"#)).unwrap();
        }
        if worker_active {
            let entry = root.join(".blkit/src/bin").join(if target == "api-worker" {
                "orders-api-worker.rs"
            } else {
                "orders-worker.rs"
            });
            let generated = fs::read_to_string(&entry).unwrap();
            assert_eq!(generated.matches("#[tokio::main]").count(), 1);
            fs::write(
                &entry,
                generated.replace("#[tokio::main]", "#[tokio::main(worker_threads = 8)]"),
            )
            .unwrap();
        }
        if target == "api-worker" {
            let entry = root.join(".blkit/src/bin/orders-api-worker.rs");
            let generated = fs::read_to_string(&entry).unwrap();
            let api_call = "let store = PostgresStore::connect_with_pool_size(url, ";
            let worker_call = "let worker_store = PostgresStore::connect_with_pool_size(url, ";
            assert_eq!(generated.matches(api_call).count(), 1);
            assert_eq!(generated.matches(worker_call).count(), 1);
            let tagged = generated
                .replace(api_call, &format!("let store = PostgresStore::connect_with_pool_size(&url.replace(\"application_name={app}\", \"application_name={app}_api\"), "))
                .replace(worker_call, &format!("let worker_store = PostgresStore::connect_with_pool_size(&url.replace(\"application_name={app}\", \"application_name={app}_worker\"), "));
            fs::write(entry, tagged).unwrap();
        }
        cargo_build(&root).unwrap();
        let url = format!("{base_url}?application_name={app}");
        if !worker_active {
            let store = PostgresStore::connect(&base_url).await.unwrap();
            store
                .register_worker(&format!("{app}_fixture"), &[("orders", "1", "route")])
                .await
                .unwrap();
        } else {
            let store = PostgresStore::connect(&base_url).await.unwrap();
            for i in 0..12 {
                let item = Instance::new(
                    &format!("{app}_{i}"),
                    "orders",
                    "1",
                    "route",
                    serde_json::json!({"input": i.to_string()}),
                );
                store.create(&item).await.unwrap();
            }
            drop(store);
        }
        let bin = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or(root.join(".blkit/target"))
            .join("debug")
            .join(binary);
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = socket.local_addr().unwrap().to_string();
        drop(socket);
        let mut command = Command::new(bin);
        command.arg(&url);
        if worker_active {
            command.args(["4", "1000"]);
        }
        if binary != "orders-worker" {
            command.arg(&bind);
        }
        if let Some(max) = api_max {
            command.env("BLKIT_API_DB_POOL_MAX", max);
        } else {
            command.env_remove("BLKIT_API_DB_POOL_MAX");
        }
        if let Some(max) = worker_max {
            command.env("BLKIT_WORKER_DB_POOL_MAX", max);
        } else {
            command.env_remove("BLKIT_WORKER_DB_POOL_MAX");
        }
        if worker_active {
            command.env("BLKIT_TEST_GATE_DIR", &root);
        }
        let mut child = Child(
            command
                .stdout(Stdio::null())
                .stderr(Stdio::from(
                    fs::File::create(root.join("stderr.log")).unwrap(),
                ))
                .spawn()
                .unwrap(),
        );
        // Keep four real worker claims active while the table lock contends with persistence.
        if worker_active {
            tokio::time::timeout(Duration::from_secs(10), async {
                while fs::read_dir(&root)
                    .unwrap()
                    .filter(|entry| {
                        entry
                            .as_ref()
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("entered-")
                    })
                    .count()
                    < 4
                {
                    if let Some(exit) = child.0.try_wait().unwrap() {
                        panic!(
                            "{target}: worker exited {exit}: {}",
                            fs::read_to_string(root.join("stderr.log")).unwrap()
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("worker did not claim four tasks despite MAX_TASKS=4");
        }
        if binary != "orders-worker" {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if std::net::TcpStream::connect(&bind).is_ok() {
                        break;
                    }
                    if let Some(exit) = child.0.try_wait().unwrap() {
                        panic!(
                            "{target}: server exited {exit}: {}",
                            fs::read_to_string(root.join("stderr.log")).unwrap()
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("generated API listener never became ready");
        }
        // A tagged, held table lock is observed in PostgreSQL before any pressure requests.
        let mut requests = Vec::new();
        let mut lock = Child(
            Command::new("docker")
                .args([
                    "exec",
                    "-i",
                    postgres.id(),
                    "psql",
                    "-U",
                    "postgres",
                    "-q",
                    "-X",
                    "-v",
                    "ON_ERROR_STOP=1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        lock.0.stdin.as_mut().unwrap().write_all(format!("BEGIN; LOCK TABLE instances IN ACCESS EXCLUSIVE MODE; /* {app}_lock */ SELECT 1;\n").as_bytes()).unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let mut observer = postgres.exec(ExecCommand::new(["psql", "-U", "postgres", "-Atc",
                    &format!("SELECT count(*) FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid JOIN pg_class c ON c.oid=l.relation WHERE a.query LIKE '%{app}_lock%' AND a.state='idle in transaction' AND l.mode='AccessExclusiveLock' AND l.granted AND c.relname='instances'"),
                ])).await.unwrap();
                let held = String::from_utf8(observer.stdout_to_vec().await.unwrap()).unwrap();
                if held.trim() == "1" { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("disposable PostgreSQL table lock was not acquired");
        if binary != "orders-worker" {
            for _ in 0..24 {
                let endpoint = format!("http://{bind}/processes/orders/1/route/instances");
                requests.push(tokio::task::spawn_blocking(move || {
                    Command::new("curl")
                        .args([
                            "-sS",
                            "--max-time",
                            "8",
                            "-H",
                            "content-type: application/json",
                            "-d",
                            "{\"input\":\"5\"}",
                            &endpoint,
                        ])
                        .output()
                        .unwrap()
                }));
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{target}: {}",
            fs::read_to_string(root.join("stderr.log")).unwrap()
        );
        let mut observer = postgres
            .exec(ExecCommand::new([
                "psql",
                "-U",
                "postgres",
                "-Atc",
                &format!("SELECT count(*) FROM pg_stat_activity WHERE application_name='{app}' OR application_name IN ('{app}_api', '{app}_worker')"),
            ]))
            .await
            .unwrap();
        let count: i64 = String::from_utf8(observer.stdout_to_vec().await.unwrap())
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let expected_min = if target == "api-worker" { 2 } else { 1 };
        assert!(
            (expected_min..=expected_max).contains(&count),
            "{target} API={api_max:?} worker={worker_max:?}: expected {expected_min}..={expected_max} sessions, got {count}"
        );
        lock.0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"COMMIT;\n\\q\n")
            .unwrap();
        drop(lock.0.stdin.take());
        tokio::time::timeout(Duration::from_secs(3), async {
            while lock.0.try_wait().unwrap().is_none() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("disposable PostgreSQL lock client did not exit");
        if target == "api-worker" {
            let mut observer = postgres.exec(ExecCommand::new([
                "psql", "-U", "postgres", "-Atc",
                &format!("SELECT count(*) FILTER (WHERE application_name='{app}_api'), count(*) FILTER (WHERE application_name='{app}_worker') FROM pg_stat_activity"),
            ])).await.unwrap();
            let roles = String::from_utf8(observer.stdout_to_vec().await.unwrap()).unwrap();
            let (api, worker) = roles.trim().split_once('|').unwrap();
            let (api, worker) = (api.parse::<i64>().unwrap(), worker.parse::<i64>().unwrap());
            let (api_limit, worker_limit) = (
                api_max.unwrap_or("4").parse::<i64>().unwrap(),
                worker_max.unwrap_or("4").parse::<i64>().unwrap(),
            );
            assert!(
                (1..=api_limit).contains(&api) && (1..=worker_limit).contains(&worker),
                "combined API={api_max:?} worker={worker_max:?}: observed API={api}, worker={worker}"
            );
        }
        for request in requests {
            let output = request.await.unwrap();
            assert!(output.status.success() && serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()
                .is_some_and(|body| body.get("id").is_some() || body.get("error").is_some()),
                "{target}: pressure request did not reach generated API route (exit {}, stderr {})", output.status, String::from_utf8_lossy(&output.stderr));
        }
        if binary != "orders-worker" {
            let endpoint = format!("http://{bind}/processes/orders/1/route/instances");
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let output = Command::new("curl")
                        .args([
                            "-sS",
                            "--max-time",
                            "3",
                            "-H",
                            "content-type: application/json",
                            "-d",
                            "{\"input\":\"5\"}",
                            &endpoint,
                        ])
                        .output()
                        .unwrap();
                    if output.status.success()
                        && serde_json::from_slice::<serde_json::Value>(&output.stdout)
                            .ok()
                            .is_some_and(|body| body.get("id").and_then(|id| id.as_str()).is_some())
                    {
                        break;
                    }
                    if let Some(exit) = child.0.try_wait().unwrap() {
                        panic!(
                            "{target}: API exited {exit}: {}",
                            fs::read_to_string(root.join("stderr.log")).unwrap()
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await
            .expect("generated API did not accept a valid request after lock release");
        }
        drop(child);
        // Each case shares only this test's disposable PostgreSQL container.
        let mut cleanup = postgres
            .exec(ExecCommand::new([
                "psql",
                "-U",
                "postgres",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                "DELETE FROM instances; DELETE FROM workers;",
            ]))
            .await
            .unwrap();
        let _ = cleanup.stdout_to_vec().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn generated_local_role_ignores_postgres_pool_settings_and_has_no_postgres_dependency() {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let root = project(
        &MANIFEST.replace(
            "build_target = \"crate\"\n",
            "build_target = \"api-worker\"\npersistence = \"local\"\n",
        ),
        &[("route.bl", SOURCE)],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let tree = Command::new("cargo")
        .args(["tree", "--edges", "normal", "--manifest-path"])
        .arg(root.join(".blkit/Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        tree.status.success(),
        "{}",
        String::from_utf8_lossy(&tree.stderr)
    );
    let graph = String::from_utf8(tree.stdout).unwrap();
    assert!(
        !graph.contains("deadpool-postgres") && !graph.contains("tokio-postgres"),
        "{graph}"
    );
    let bin = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or(root.join(".blkit/target"))
        .join("debug/orders-api-worker");
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = socket.local_addr().unwrap().to_string();
    drop(socket);
    let mut child = Command::new(bin)
        .arg(root.join("local.db"))
        .args(["2", &bind])
        .env("BLKIT_API_DB_POOL_MAX", "bad")
        .env("BLKIT_WORKER_DB_POOL_MAX", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        child.try_wait().unwrap().is_none(),
        "local role rejected remote-only settings"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn generated_role_executables_smoke() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }";
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
    for (target, persistence) in [
        ("api-worker", "local"),
        ("api-worker", "remote"),
        ("api-worker-split", ""),
    ] {
        let settings = if persistence.is_empty() {
            String::new()
        } else {
            format!("persistence = {persistence:?}\n")
        };
        let root = project(
            &MANIFEST.replace(
                "build_target = \"crate\"\n",
                &format!("build_target = {target:?}\n{settings}"),
            ),
            &[("route.bl", source)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        cargo_build(&root).unwrap();
        if target == "api-worker-split" {
            let api = root.join(".blkit/api/Cargo.toml");
            let output = std::process::Command::new("cargo")
                .args(["build", "--manifest-path"])
                .arg(api)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let binaries = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or(root.join(".blkit/target"))
            .join("debug");
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = socket.local_addr().unwrap().to_string();
        drop(socket);
        let database = root.join("local.db");
        let mut children = Vec::new();
        if target == "api-worker-split" {
            children.push(Child(
                std::process::Command::new(binaries.join("orders-worker"))
                    .args([&url, "2", "1000"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            ));
        }
        let mut server = std::process::Command::new(binaries.join("orders-api-worker"));
        if target == "api-worker-split" {
            server = std::process::Command::new(binaries.join("orders-api"));
            server.args([&url, &bind]);
        } else if persistence == "local" {
            server.arg(&database).args(["2", &bind]);
        } else {
            server.args([&url, "2", "1000", &bind]);
        }
        children.push(Child(
            server
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        ));
        let endpoint = format!("http://{bind}/processes/orders/1/route/instances");
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let output = std::process::Command::new("curl")
                    .args([
                        "-sS",
                        "-H",
                        "content-type: application/json",
                        "-d",
                        "{\"input\":\"5\"}",
                        &endpoint,
                    ])
                    .output()
                    .unwrap();
                if output.status.success()
                    && serde_json::from_slice::<serde_json::Value>(&output.stdout)
                        .ok()
                        .and_then(|v| v.get("id").cloned())
                        .is_some()
                {
                    break serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("generated server did not accept request");
        let status_url = format!(
            "http://{bind}/instances/{}",
            response["id"].as_str().unwrap()
        );
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let output = std::process::Command::new("curl")
                    .args(["-fsS", &status_url])
                    .output()
                    .unwrap();
                let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                if row["status"] == "completed" {
                    assert_eq!(row["result"], "5", "{target}: {row}");
                    break;
                }
                assert!(
                    matches!(row["status"].as_str(), Some("pending" | "running")),
                    "{target}: {row}"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("generated worker did not complete request");
        drop(children);
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn generated_remote_workers_drain_active_claims_without_claiming_backlog() {
    use blkit_core::{postgres_store::PostgresStore, runtime::Instance};
    use std::{process::Stdio, time::Duration};

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } } process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    for (target, persistence, binary) in [
        ("worker-only", "", "orders-worker"),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker",
        ),
    ] {
        let root = project(
            &MANIFEST.replace(
                "build_target = \"crate\"\n",
                &format!("build_target = {target:?}\n{persistence}"),
            ),
            &[("route.bl", source)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        let lib = root.join(".blkit/src/lib.rs");
        let generated = fs::read_to_string(&lib).unwrap();
        assert!(generated.contains("pub fn named_graph_definitions()"));
        // Instrument only the generated fixture's task so the real binary can be drained mid-claim.
        fs::write(
            &lib,
            format!(
                "{}\n{}",
                generated.replacen("pub fn named_graph_definitions()", "fn compiled_graph_definitions()", 1),
                r#"pub fn named_graph_definitions() -> Vec<blkit_core::compiled_graph::GraphDefinition> {
    let mut graphs = compiled_graph_definitions();
    let task = graphs[0].nodes.iter_mut().find(|node| node.name == "echo").unwrap();
    task.kind = blkit_core::compiled_graph::GraphNodeKind::Task(std::sync::Arc::new(|input, _| {
        let gate = std::path::PathBuf::from(std::env::var("BLKIT_TEST_GATE_DIR").unwrap());
        let name = if input["input"] == "1" { "slow" } else { "fast" };
        std::fs::write(gate.join(format!("{name}-entered")), "").unwrap();
        while !gate.join(format!("{name}-release")).exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(input["input"].clone())
    }));
    graphs
}"#
            ),
        )
        .unwrap();
        cargo_build(&root).unwrap();
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
        let store = PostgresStore::connect(&url).await.unwrap();
        for (id, input) in [("slow", "1"), ("fast", "2"), ("backlog", "3")] {
            let mut item = Instance::new(
                id,
                "orders",
                "1",
                "route",
                serde_json::json!({"input": input}),
            );
            item.created_at = match id {
                "slow" => 1,
                "fast" => 2,
                _ => 3,
            };
            store.create(&item).await.unwrap();
        }
        let path = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or(root.join(".blkit/target"))
            .join("debug")
            .join(binary);
        let mut command = std::process::Command::new(path);
        command.args([&url, "2", "5000"]);
        command.env("BLKIT_TEST_GATE_DIR", &root);
        if target == "api-worker" {
            let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = socket.local_addr().unwrap().to_string();
            drop(socket);
            command.arg(address);
        }
        let stderr = root.join("worker-stderr.log");
        let mut child = Child(
            command
                .stdout(Stdio::null())
                .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
                .spawn()
                .unwrap(),
        );
        let worker_id = format!("worker-{}", child.0.id());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(exit) = child.0.try_wait().unwrap() {
                    panic!(
                        "{target}: generated worker exited early ({exit}): {}",
                        fs::read_to_string(&stderr).unwrap()
                    );
                }
                if root.join("slow-entered").exists() && root.join("fast-entered").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{target}: task bodies did not enter: {}",
                fs::read_to_string(&stderr).unwrap()
            )
        });
        assert!(!root.join("slow-release").exists() && !root.join("fast-release").exists());
        let mut observer = postgres
            .exec(
                testcontainers_modules::testcontainers::core::ExecCommand::new([
                    "psql",
                    "-U",
                    "postgres",
                    "-Atc",
                    "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()",
                ]),
            )
            .await
            .unwrap();
        let output = observer.stdout_to_vec().await.unwrap();
        let connections: i64 = String::from_utf8(output).unwrap().trim().parse().unwrap();
        // The observer and test store use one session each; generated roles open lazily up to four each.
        let (min, max) = if target == "api-worker" {
            (4, 10)
        } else {
            (3, 6)
        };
        assert!(
            (min..=max).contains(&connections),
            "{target}: expected {min}..={max} bounded sessions, got {connections}"
        );
        let slow = store.get("slow").await.unwrap().unwrap();
        let fast = store.get("fast").await.unwrap().unwrap();
        assert_eq!(
            slow.instance.status, "running",
            "{target}: slow task blocked"
        );
        assert_eq!(
            fast.instance.status, "running",
            "{target}: fast task blocked"
        );
        assert!(
            store.drain(&worker_id).await.unwrap(),
            "{target}: worker not registered"
        );
        let drained_lease = store
            .get("slow")
            .await
            .unwrap()
            .unwrap()
            .lease_until
            .unwrap();
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let slow = store.get("slow").await.unwrap().unwrap();
                if slow.lease_until.unwrap() > drained_lease {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("active claim must renew its lease while draining");
        assert_eq!(
            store.get("fast").await.unwrap().unwrap().instance.status,
            "running",
            "{target}: fast task still blocked after drain"
        );
        fs::write(root.join("fast-release"), "").unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if store.get("fast").await.unwrap().unwrap().instance.status == "completed" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("fast claim must finish while draining");
        assert_eq!(
            store.get("slow").await.unwrap().unwrap().instance.status,
            "running",
            "{target}"
        );
        let backlog = store.get("backlog").await.unwrap().unwrap();
        assert_eq!(
            backlog.instance.status, "pending",
            "{target}: no refill after drain"
        );
        assert!(
            backlog.owner_id.is_none(),
            "{target}: backlog must remain unclaimed"
        );
        fs::write(root.join("slow-release"), "").unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(exit) = child.0.try_wait().unwrap() {
                    break exit;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("generated worker must exit after its slow claim finishes");
        assert!(exit.success(), "{target}: {exit}");
        assert_eq!(
            store.get("slow").await.unwrap().unwrap().instance.status,
            "completed",
            "{target}"
        );
        assert!(
            store.get_worker(&worker_id).await.unwrap().is_none(),
            "{target}: must unregister"
        );
        assert_eq!(
            store.get("backlog").await.unwrap().unwrap().instance.status,
            "pending",
            "{target}"
        );
        drop(child);
        drop(store);
        drop(postgres);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn combined_and_split_targets_build_their_role_binaries() {
    for (target, persistence, bin) in [
        (
            "api-worker",
            "persistence = \"local\"\n",
            "orders-api-worker.rs",
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            "orders-api-worker.rs",
        ),
        ("worker-only", "", "orders-worker.rs"),
        ("api-worker-split", "", "orders-worker.rs"),
    ] {
        let root = project(
            &MANIFEST.replace(
                "build_target = \"crate\"\n",
                &format!("build_target = {target:?}\n{persistence}"),
            ),
            &[("route.bl", SOURCE)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        assert!(root.join(".blkit/src/bin").join(bin).exists(), "{target}");
        cargo_build(&root).unwrap();
        if target == "api-worker-split" {
            let api = root.join(".blkit/api/Cargo.toml");
            let manifest = fs::read_to_string(&api).unwrap();
            assert!(
                manifest.contains("api-server") && !manifest.contains("\"worker\""),
                "{manifest}"
            );
            let output = std::process::Command::new("cargo")
                .args(["build", "--manifest-path"])
                .arg(api)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn new_targets_accept_valid_backend_and_source_combinations() {
    for (target, persistence, sources) in [
        ("api-only", "", false),
        ("api-only", "", true),
        ("api-worker", "persistence = \"local\"\n", true),
        ("api-worker", "persistence = \"remote\"\n", true),
        ("api-worker-split", "", true),
        ("worker-only", "", true),
    ] {
        let manifest = MANIFEST.replace(
            "build_target = \"crate\"\n",
            &format!("build_target = {target:?}\n{persistence}"),
        );
        let files = if sources {
            vec![("route.bl", SOURCE)]
        } else {
            vec![]
        };
        let root = project(&manifest, &files);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(["transpile"])
            .arg(&root)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(root.join(".blkit/Cargo.toml").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn new_targets_reject_invalid_backend_and_missing_worker_sources() {
    for (target, persistence, sources, expected) in [
        ("api-worker", "", true, "persistence"),
        (
            "api-worker",
            "persistence = \"invalid\"\n",
            true,
            "persistence",
        ),
        (
            "api-only",
            "persistence = \"remote\"\n",
            false,
            "persistence",
        ),
        (
            "worker-only",
            "persistence = \"remote\"\n",
            true,
            "persistence",
        ),
        ("api-worker-split", "", false, "no .bl files"),
        (
            "api-worker",
            "persistence = \"local\"\n",
            false,
            "no .bl files",
        ),
        ("worker-only", "", false, "no .bl files"),
    ] {
        let manifest = MANIFEST.replace(
            "build_target = \"crate\"\n",
            &format!("build_target = {target:?}\n{persistence}"),
        );
        let files = if sources {
            vec![("route.bl", SOURCE)]
        } else {
            vec![]
        };
        let root = project(&manifest, &files);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(["transpile"])
            .arg(&root)
            .output()
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && diagnostic.contains(expected),
            "{target}: {diagnostic}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn discovers_nested_bl_files_but_not_generated_or_hidden_files() {
    let root = project(
        MANIFEST,
        &[
            ("a.bl", SOURCE),
            ("src/deep/b.bl", SOURCE),
            (".blkit/generated.bl", "bad"),
            ("target/ignored.bl", "bad"),
            (".hidden/ignored.bl", "bad"),
        ],
    );
    let loaded = blkit::project::Project::load(&root).unwrap();
    assert_eq!(
        loaded.sources,
        [root.join("a.bl"), root.join("src/deep/b.bl")]
    );
    assert_eq!(loaded.build_target, "crate");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extension_aliases_must_be_usable_as_rust_identifiers() {
    for alias in ["9payments", "pay-ments"] {
        let root = project(
            &format!("{MANIFEST}[dependencies]\n{alias} = \"0.1.0\"\n"),
            &[("a.bl", SOURCE)],
        );
        let error = blkit::project::Project::load(&root).err().unwrap();
        assert!(error.contains("dependency"), "{alias}: {error}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn project_requires_one_valid_target_and_matching_toolchain() {
    for (label, manifest, expected) in [
        (
            "absent",
            MANIFEST.replace("build_target = \"crate\"\n", ""),
            "build_target",
        ),
        (
            "multiple",
            MANIFEST.replace("\"crate\"", "[\"crate\", \"worker\"]"),
            "build_target",
        ),
        (
            "unknown",
            MANIFEST.replace("\"crate\"", "\"unknown\""),
            "build_target",
        ),
        (
            "mismatch",
            MANIFEST.replace("\"0.1.0\"", "\"9.9.9\""),
            "blkit version",
        ),
        (
            "sources",
            format!("{MANIFEST}sources = [\"a.bl\"]\n"),
            "sources",
        ),
    ] {
        let root = project(&manifest, &[("a.bl", SOURCE)]);
        let error = blkit::project::Project::load(&root).err().unwrap();
        assert!(error.contains(expected), "{label}: {error}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn project_resolves_cross_file_types_and_decision_tasks_in_any_file_order() {
    let source = "namespace orders; version \"1\"; Order = {value: Number};\nstart_event start { output input: Order; } end_event done { input result: Order; } end_event checked_done { input result: Bool; } decision_task echo { input input: Order; output result: Order = value; literal_expression value { output result: Order; expression input; } } decision_task flag { input input: Order; output result: Bool = value; literal_expression value { output result: Bool; expression true; } }";
    let processes = "namespace orders; version \"1\"; process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; } process checked { flow start -> flag; flow flag -> checked_done; bind start.input -> flag.input; bind flag.result -> checked_done.result; }";
    let root = project(
        MANIFEST,
        &[("a-process.bl", &processes), ("z-declarations.bl", &source)],
    );
    let programs = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .unwrap();
    assert_eq!(programs.len(), 1);
    assert_eq!(programs[0].processes.len(), 2);
    assert_eq!(programs[0].peer_nodes.len(), 3);
    assert_eq!(programs[0].decisions.len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_resolves_subprocesses_across_same_scope_files() {
    let parent = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } subprocess called { process child; input input: Number; output result: Number; } process parent { flow start -> called; flow called -> done; bind start.input -> called.input; bind called.result -> done.result; }";
    let child = "namespace orders; version \"1\"; process child { flow start -> done; bind start.input -> done.result; }";
    for (a, b) in [(parent, child), (child, parent)] {
        let root = project(MANIFEST, &[("a.bl", a), ("z.bl", b)]);
        let programs = blkit::project::Project::load(&root)
            .unwrap()
            .programs()
            .unwrap();
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].processes.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn server_and_worker_build_link_cross_file_subprocesses() {
    let parent = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } subprocess called { process child; input input: Number; output result: Number; } process parent { flow start -> called; flow called -> done; bind start.input -> called.input; bind called.result -> done.result; }";
    let child = "namespace orders; version \"1\"; process child { flow start -> done; bind start.input -> done.result; }";
    for target in ["server", "worker"] {
        let root = project(
            &MANIFEST
                .replace("name = \"orders\"", "name = \"subprocess_fixture\"")
                .replace("\"crate\"", &format!("\"{target}\"")),
            &[("a-parent.bl", parent), ("z-child.bl", child)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        cargo_build(&root).unwrap();
        fs::remove_file(root.join("z-child.bl")).unwrap();
        assert!(
            blkit::project::Project::load(&root)
                .unwrap()
                .transpile()
                .unwrap_err()
                .contains("unknown subprocess: child")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn project_rejects_subprocesses_across_namespaces_or_versions() {
    let parent = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } subprocess called { process child; input input: Number; output result: Number; } process parent { flow start -> called; flow called -> done; bind start.input -> called.input; bind called.result -> done.result; }";
    for child_header in [
        "namespace other; version \"1\";",
        "namespace orders; version \"2\";",
    ] {
        let child = format!(
            "{child_header} start_event start {{ output input: Number; }} end_event done {{ input result: Number; }} process child {{ flow start -> done; bind start.input -> done.result; }}"
        );
        let root = project(MANIFEST, &[("a.bl", parent), ("z.bl", &child)]);
        let error = blkit::project::Project::load(&root)
            .unwrap()
            .programs()
            .unwrap_err();
        assert!(
            error.contains("a.bl") && error.contains("unknown subprocess: child"),
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn project_rejects_duplicates_with_both_paths_and_keeps_versions_isolated() {
    let root = project(
        MANIFEST,
        &[
            (
                "one.bl",
                "namespace orders; version \"1\"; Shared = {value: Number};\n",
            ),
            (
                "two.bl",
                "namespace orders; version \"1\"; Shared = {value: Bool};\n",
            ),
        ],
    );
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .err()
        .unwrap();
    assert!(
        error.contains("one.bl") && error.contains("two.bl") && error.contains("Shared"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
    let root = project(
        MANIFEST,
        &[
            (
                "one.bl",
                "namespace orders; version \"1\"; Shared = {value: Number};\n",
            ),
            (
                "two.bl",
                "namespace orders; version \"2\"; start_event start { output input: Shared; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }",
            ),
        ],
    );
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .programs()
        .err()
        .unwrap();
    assert!(
        error.contains("two.bl") && error.contains("Shared"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_builds_a_reusable_library_from_multiple_scopes_and_cross_file_tasks() {
    let process = "namespace orders; version \"1\"; process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    let definitions = "namespace orders; version \"1\"; Order = {value: Number};\nstart_event start { output input: Order; } end_event done { input result: Order; } decision_task echo { input input: Order; output result: Order = value; literal_expression value { output result: Order; expression input; } }";
    let other = "namespace orders; version \"2\"; start_event start { output input: Number; } end_event done { input result: Number; } process other { flow start -> done; bind start.input -> done.result; }";
    let root = project(
        MANIFEST,
        &[
            ("a-process.bl", process),
            ("src/definitions.bl", definitions),
            ("src/v2.bl", other),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = \"consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\norders = {{ path = {:?} }}\nserde_json = \"1\"\n", root.join(".blkit").to_str().unwrap())).unwrap();
    fs::write(consumer.join("src/lib.rs"), "#[test] fn imported_processes_run() { let defs = orders::named_graph_definitions(); assert_eq!(defs.len(), 2); let graph = defs.into_iter().find(|d| d.version == \"1\").unwrap(); let input = serde_json::json!({\"input\": {\"value\": \"2\"}}); let mut checkpoint = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut checkpoint).unwrap(), serde_json::json!({\"value\":\"2\"})); }\n").unwrap();
    let output = std::process::Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join(".blkit/target")),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_string_helpers_are_available_to_generated_crate_and_consumer() {
    let root = project(
        MANIFEST,
        &[(
            "strings.bl",
            "namespace orders; version \"1\"; start_event start { output input: String; } end_event done { input result: String; } decision_task first { input input: String; output result: String = value; literal_expression value { output result: String; expression charAt(input, 1); } } decision_task check { input input: String; output result: Bool = value; literal_expression value { output result: Bool; expression matches(input, input); } } process route { flow start -> first; flow first -> done; bind start.input -> first.input; bind first.result -> done.result; }",
        )],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = \"string_consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\norders = {{ path = {:?} }}\nserde_json = \"1\"\n", root.join(".blkit").to_str().unwrap())).unwrap();
    fs::write(consumer.join("src/lib.rs"), "#[test] fn generated_strings() { assert_eq!(orders::scope_0::first(\"éx\".into()).unwrap(), \"é\"); assert!(orders::scope_0::check(\"[\".into()).is_err()); let graph = orders::named_graph_definitions().remove(0); let input = serde_json::json!({\"input\":\"éx\"}); let mut checkpoint = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut checkpoint).unwrap(), serde_json::json!(\"é\")); }\n").unwrap();
    let output = std::process::Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join(".blkit/target")),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_locks_local_dependency_versions_until_explicit_update() {
    let manifest =
        format!("{MANIFEST}[dependencies]\nextra = {{ version = \"0.1.0\", path = \"extra\" }}\n");
    let local = "[package]\nname = \"extra\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
    let root = project(
        &manifest,
        &[
            ("source.bl", SOURCE),
            ("extra/Cargo.toml", local),
            ("extra/src/lib.rs", "pub fn noop() {}"),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let lockfile = root.join(".blkit/Cargo.lock");
    let locked = fs::read_to_string(&lockfile).unwrap();
    assert!(locked.contains("name = \"extra\"\nversion = \"0.1.0\""));
    fs::write(
        root.join("extra/Cargo.toml"),
        local.replace("0.1.0", "0.1.1"),
    )
    .unwrap();
    fs::write(
        root.join("blkit.toml"),
        manifest.replace("version = \"0.1.0\", path", "version = \"0.1.1\", path"),
    )
    .unwrap();
    let updated = blkit::project::Project::load(&root).unwrap();
    updated.transpile().unwrap();
    let error = cargo_build(&root).unwrap_err();
    assert!(
        error.contains("lock") || error.contains("locked"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&lockfile).unwrap(), locked);
    updated.update().unwrap();
    updated.transpile().unwrap();
    cargo_build(&root).unwrap();
    assert!(
        fs::read_to_string(&lockfile)
            .unwrap()
            .contains("name = \"extra\"\nversion = \"0.1.1\"")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_refreshes_lockfile_when_an_unused_dependency_version_changes() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let source = SOURCE;
    let extension_manifest = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde_json = \"1\"\n";
    let descriptor = "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Number\"\noutput = \"Number\"\n";
    let extension = "pub async fn charge(input: serde_json::Value) -> Result<serde_json::Value, String> { Ok(input) }\n";
    let root = project(
        &manifest,
        &[
            ("route.bl", source),
            ("payments/Cargo.toml", extension_manifest),
            ("payments/blkit-tasks.toml", descriptor),
            ("payments/src/lib.rs", extension),
        ],
    );
    let initial = blkit::project::Project::load(&root).unwrap();
    initial.transpile().unwrap();
    initial.update().unwrap();
    let locked = fs::read_to_string(root.join(".blkit/Cargo.lock")).unwrap();
    fs::write(
        root.join("blkit.toml"),
        manifest.replace("version = \"0.1.0\", path", "version = \"0.1.1\", path"),
    )
    .unwrap();
    fs::write(
        root.join("payments/Cargo.toml"),
        extension_manifest.replace("0.1.0", "0.1.1"),
    )
    .unwrap();
    let updated = blkit::project::Project::load(&root).unwrap();
    updated.transpile().unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".blkit/Cargo.lock")).unwrap(),
        locked
    );
    updated.update().unwrap();
    updated.transpile().unwrap();
    assert!(
        fs::read_to_string(root.join(".blkit/Cargo.lock"))
            .unwrap()
            .contains("name = \"payments\"\nversion = \"0.1.1\"")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_extension_call_fails_before_resolving_missing_dependency() {
    let root = project(
        &format!(
            "{MANIFEST}[dependencies]\nmissing = {{ version = \"0.1.0\", path = \"not-there\" }}\n"
        ),
        &[(
            "source.bl",
            "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { node work = task missing.charge(start.input); flow start -> done; bind start.input -> done.result; }",
        )],
    );
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap_err();
    assert!(error.contains("invalid process statement"), "{error}");
    assert!(!root.join(".blkit/Cargo.toml").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_only_tracks_generated_lockfile_and_ignore_rules() {
    let root = project(MANIFEST, &[("source.bl", SOURCE)]);
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let output = std::process::Command::new("git")
        .args(["status", "--short", "--untracked-files=all"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let files = String::from_utf8(output.stdout).unwrap();
    assert!(files.contains(".blkit/Cargo.lock"), "{files}");
    assert!(files.contains(".blkit/.gitignore"), "{files}");
    assert!(
        !files.contains(".blkit/src/") && !files.contains(".blkit/Cargo.toml"),
        "{files}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolved_extension_crate_exposes_typed_task_descriptors() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let root = project(
        &manifest,
        &[
            ("source.bl", SOURCE),
            (
                "payments/Cargo.toml",
                "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
            ("payments/src/lib.rs", "pub fn placeholder() {}"),
            (
                "payments/blkit-tasks.toml",
                "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n",
            ),
        ],
    );
    let project = blkit::project::Project::load(&root).unwrap();
    project.transpile().unwrap();
    let tasks = project.extensions().unwrap();
    let charge = &tasks["payments.charge"];
    assert_eq!(charge.function, "charge");
    assert_eq!(charge.input.to_string(), "Order");
    assert_eq!(charge.output.to_string(), "Receipt");
    assert!(!tasks.contains_key("payments.missing"));
    fs::remove_file(root.join("payments/blkit-tasks.toml")).unwrap();
    assert!(
        project
            .extensions()
            .err()
            .unwrap()
            .contains("blkit-tasks.toml")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extension_descriptor_rejects_malformed_function_and_types() {
    for (label, descriptor) in [
        (
            "function",
            "[[tasks]]\nname = \"charge\"\nfunction = \"charge(); bad\"\ninput = \"Order\"\noutput = \"Receipt\"\n",
        ),
        (
            "type",
            "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"List<\"\noutput = \"Receipt\"\n",
        ),
        (
            "duplicate",
            "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n",
        ),
    ] {
        let manifest = format!(
            "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
        );
        let root = project(
            &manifest,
            &[
                ("source.bl", SOURCE),
                (
                    "payments/Cargo.toml",
                    "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("payments/src/lib.rs", "pub fn placeholder() {}"),
                ("payments/blkit-tasks.toml", descriptor),
            ],
        );
        let project = blkit::project::Project::load(&root).unwrap();
        project.transpile().unwrap();
        let error = project.extensions().err().unwrap();
        assert!(
            error.contains("charge") || error.contains("List<"),
            "{label}: {error}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cross_file_legacy_extension_calls_fail_before_cargo_metadata() {
    for statement in [
        "node paid = task payments.charge(start.input);",
        "node batch = task payments.increment each start.input parallel;",
        "node batch = task payments.increment each start.input sequential;",
        "node repeated = task payments.increment(start.input) repeat_post(repeated < 3) max_iterations 2;",
    ] {
        let source = format!(
            "namespace orders; version \"1\"; process route {{ {statement} flow start -> done; bind start.input -> done.result; }}"
        );
        let peers = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; }";
        let root = project(
            &format!(
                "{MANIFEST}[dependencies]\npayments = {{ version = \"1\", path = \"missing\" }}\n"
            ),
            &[("a.bl", &source), ("z.bl", peers)],
        );
        let error = blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap_err();
        assert!(
            error.contains("invalid process statement") && error.contains("a.bl"),
            "{error}"
        );
        assert!(!root.join(".blkit/Cargo.toml").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn unused_dependency_compilation_errors_are_reported_by_cargo_build() {
    let root = project(
        &format!(
            "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
        ),
        &[
            ("source.bl", SOURCE),
            (
                "payments/Cargo.toml",
                "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
            ("payments/src/lib.rs", "this is not Rust"),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    let error = cargo_build(&root).unwrap_err();
    assert!(
        error.contains("payments") && error.contains("cargo build failed"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn switching_project_target_keeps_lockfile_and_only_selected_binary() {
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }";
    let root = project(MANIFEST, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let locked = fs::read(root.join(".blkit/Cargo.lock")).unwrap();
    fs::write(
        root.join("blkit.toml"),
        MANIFEST.replace("\"crate\"", "\"worker\""),
    )
    .unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-worker.rs").exists());
    assert_eq!(fs::read(root.join(".blkit/Cargo.lock")).unwrap(), locked);
    fs::write(
        root.join("blkit.toml"),
        MANIFEST.replace("\"crate\"", "\"server\""),
    )
    .unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-server.rs").exists());
    assert!(!root.join(".blkit/src/bin/orders-worker.rs").exists());
    assert_eq!(fs::read(root.join(".blkit/Cargo.lock")).unwrap(), locked);
    fs::write(root.join("blkit.toml"), MANIFEST).unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(!root.join(".blkit/src/bin/orders-worker.rs").exists());
    assert!(!root.join(".blkit/src/bin/orders-server.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

fn http_request(port: u16, method: &str, path: &str, body: &str) -> serde_json::Value {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        time::Duration,
    };
    let mut connection = TcpStream::connect(("127.0.0.1", port)).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(connection, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    connection.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.1 202"),
        "{response}"
    );
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn renaming_project_removes_stale_generated_binary() {
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }";
    let manifest = MANIFEST.replace("\"crate\"", "\"worker\"");
    let root = project(&manifest, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-worker.rs").exists());
    fs::write(
        root.join("blkit.toml"),
        manifest.replace("name = \"orders\"", "name = \"purchases\""),
    )
    .unwrap();
    let renamed = blkit::project::Project::load(&root).unwrap();
    renamed.update().unwrap();
    renamed.transpile().unwrap();
    assert!(root.join(".blkit/src/bin/purchases-worker.rs").exists());
    assert!(!root.join(".blkit/src/bin/orders-worker.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_binaries_configure_logging_before_runtime_and_report_failures() {
    use std::process::Command;
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }";
    for role in ["server", "worker"] {
        let root = project(
            &MANIFEST
                .replace("name = \"orders\"", "name = \"logging_fixture\"")
                .replace("\"crate\"", &format!("\"{role}\"")),
            &[("route.bl", source)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        cargo_build(&root).unwrap();
        let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
            || root.join(format!(".blkit/target/debug/logging_fixture-{role}")),
            |target| PathBuf::from(target).join(format!("debug/logging_fixture-{role}")),
        );
        let args: Vec<_> = if role == "server" {
            vec![root.join("local.db").to_string_lossy().into_owned()]
        } else {
            vec!["postgres://user:secret@127.0.0.1:1/orders".into()]
        };
        let invalid = Command::new(&binary)
            .args(&args)
            .env("BLKIT_LOG_OUTPUTS", "file")
            .env_remove("BLKIT_LOG_FILE")
            .output()
            .unwrap();
        assert!(!invalid.status.success());
        let stderr = String::from_utf8_lossy(&invalid.stderr);
        assert!(stderr.contains("BLKIT_LOG_FILE"), "{role}: {stderr}");
        assert!(
            !root.join("local.db").exists(),
            "runtime started before logging"
        );
        let fatal_args = if role == "server" {
            vec![root.to_string_lossy().into_owned()]
        } else {
            args.clone()
        };
        let fatal = Command::new(&binary)
            .args(&fatal_args)
            .env_remove("BLKIT_LOG_OUTPUTS")
            .env_remove("BLKIT_LOG_LEVEL")
            .output()
            .unwrap();
        assert!(!fatal.status.success());
        let stdout = String::from_utf8_lossy(&fatal.stdout);
        assert!(
            stdout.contains("ERROR") && stdout.contains("error"),
            "{role}: {stdout}"
        );
        assert!(!stdout.contains("secret"), "{stdout}");
        assert!(
            stdout.contains(if role == "server" {
                "open store"
            } else {
                "connect to postgres"
            }),
            "{role}: {stdout}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn project_server_executes_compiled_process_over_loopback_rest() {
    use std::{
        net::{TcpListener, TcpStream},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } } process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    let root = project(
        &MANIFEST
            .replace("name = \"orders\"", "name = \"rest_fixture\"")
            .replace("\"crate\"", "\"server\""),
        &[("route.bl", source)],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join(".blkit/target/debug/rest_fixture-server"),
        |target| PathBuf::from(target).join("debug/rest_fixture-server"),
    );
    assert!(
        binary.exists(),
        "missing server binary: {}",
        binary.display()
    );
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(binary)
        .arg(root.join("local.db"))
        .arg("2")
        .arg(format!("127.0.0.1:{port}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "server failed to listen"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let id = http_request(
        port,
        "POST",
        "/processes/orders/1/route/instances",
        "{\"input\":\"7\"}",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = loop {
        let item = http_request(port, "GET", &format!("/instances/{id}"), "");
        if item["status"] == "completed" {
            break item;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "{item}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(result["result"], serde_json::json!("7"));
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("INFO") && stdout.contains("listening"),
        "{stdout}"
    );
    assert!(!stdout.contains("\"7\""), "{stdout}");
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_worker_binary_claims_only_its_compiled_process_version() {
    use blkit_core::{
        compiled_graph::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
        postgres_store::PostgresStore,
        runtime::Instance,
    };
    use serde_json::json;
    use std::{
        process::{Command, Stdio},
        sync::Arc,
        time::Duration,
    };
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{ImageExt, runners::AsyncRunner},
    };
    let manifest = MANIFEST
        .replace("name = \"orders\"", "name = \"worker_fixture\"")
        .replace("\"crate\"", "\"worker\"");
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } } process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    let root = project(&manifest, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join(".blkit/target/debug/worker_fixture-worker"),
        |target| PathBuf::from(target).join("debug/worker_fixture-worker"),
    );
    assert!(
        binary.exists(),
        "missing worker binary: {}",
        binary.display()
    );
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    let store = PostgresStore::connect(&url).await.unwrap();
    let graph = GraphDefinition {
        namespace: "orders",
        version: "1",
        name: "route",
        retry: None,
        deadline: None,
        decode_input: Box::new(Ok),
        nodes: vec![
            GraphNode {
                name: "start",
                kind: GraphNodeKind::Start,
            },
            GraphNode {
                name: "echo",
                kind: GraphNodeKind::Task(Arc::new(|input, _| Ok(input["input"].clone()))),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "echo",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "echo",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["echo"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    for (id, version) in [("matched", "1"), ("other-version", "2")] {
        let mut item = Instance::new(id, "orders", version, "route", json!({"input":"7"}));
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let mut child = Command::new(binary)
        .arg(&url)
        .args(["32", "5000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let item = store.get("matched").await.unwrap().unwrap();
            if item.instance.status == "completed" {
                break;
            }
            assert_ne!(item.instance.status, "failed", "{:?}", item.instance.error);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.get("matched").await.unwrap().unwrap().instance.result,
        Some(json!("7"))
    );
    assert_eq!(
        store
            .get("other-version")
            .await
            .unwrap()
            .unwrap()
            .instance
            .status,
        "pending"
    );
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("INFO") && stdout.contains("ready"),
        "{stdout}"
    );
    assert!(!stdout.contains("\"7\""), "{stdout}");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_requires_at_least_one_discovered_source() {
    let root = project(MANIFEST, &[(".hidden/ignored.bl", SOURCE)]);
    let error = blkit::project::Project::load(&root).err().unwrap();
    assert!(error.contains("no .bl files"), "{error}");
    fs::remove_dir_all(root).unwrap();
}
