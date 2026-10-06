use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
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
    let source = "namespace orders; version \"1\"; type Order:\n  value: Number;\nstart_event start { output input: Order; } end_event done { input result: Order; } end_event checked_done { input result: Bool; } decision_task echo { input input: Order; output result: Order = value; literal_expression value { output result: Order; expression input; } } decision_task flag { input input: Order; output result: Bool = value; literal_expression value { output result: Bool; expression true; } }";
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
            &MANIFEST.replace("\"crate\"", &format!("\"{target}\"")),
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
                "namespace orders; version \"1\"; type Shared:\n  value: Number;\n",
            ),
            (
                "two.bl",
                "namespace orders; version \"1\"; type Shared:\n  value: Bool;\n",
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
                "namespace orders; version \"1\"; type Shared:\n  value: Number;\n",
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
    let definitions = "namespace orders; version \"1\"; type Order:\n  value: Number;\nstart_event start { output input: Order; } end_event done { input result: Order; } decision_task echo { input input: Order; output result: Order = value; literal_expression value { output result: Order; expression input; } }";
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
            &MANIFEST.replace("\"crate\"", &format!("\"{role}\"")),
            &[("route.bl", source)],
        );
        blkit::project::Project::load(&root)
            .unwrap()
            .transpile()
            .unwrap();
        cargo_build(&root).unwrap();
        let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
            || root.join(format!(".blkit/target/debug/orders-{role}")),
            |target| PathBuf::from(target).join(format!("debug/orders-{role}")),
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
        &MANIFEST.replace("\"crate\"", "\"server\""),
        &[("route.bl", source)],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join(".blkit/target/debug/orders-server"),
        |target| PathBuf::from(target).join("debug/orders-server"),
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
    use blkit::{
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
    let manifest = MANIFEST.replace("\"crate\"", "\"worker\"");
    let source = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } } process route { flow start -> echo; flow echo -> done; bind start.input -> echo.input; bind echo.result -> done.result; }";
    let root = project(&manifest, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .transpile()
        .unwrap();
    cargo_build(&root).unwrap();
    let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join(".blkit/target/debug/orders-worker"),
        |target| PathBuf::from(target).join("debug/orders-worker"),
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
