use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

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

const SOURCE: &str = "namespace orders\nversion \"1\"\n";
const MANIFEST: &str =
    "[project]\nname = \"orders\"\nblkit = \"0.1.0\"\nbuild_target = \"crate\"\n";

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
fn project_resolves_cross_file_types_tasks_and_decisions_in_any_file_order() {
    let header = "namespace orders\nversion \"1\"\n";
    let source = format!(
        "{header}type Order:\n  value: Number\ntask echo(input: Order) -> Order:\n  return input\ndecision flag(input: Order) -> Bool:\n  node result: Bool = literal true\n  output result\n"
    );
    let processes = format!(
        "{header}process route(input: Order) -> Order:\n  node start = start\n  node value = task echo(input)\n  node done = end\n  link start -> value\n  link value -> done(value)\nprocess checked(input: Order) -> Bool:\n  node start = start\n  node answer = business_rule flag(input)\n  node done = end\n  link start -> answer\n  link answer -> done(answer)\n"
    );
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
    assert_eq!(programs[0].tasks[0].name, "echo");
    assert_eq!(programs[0].decisions[0].name, "flag");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_rejects_duplicates_with_both_paths_and_keeps_versions_isolated() {
    let root = project(
        MANIFEST,
        &[
            (
                "one.bl",
                "namespace orders\nversion \"1\"\ntype Shared:\n  value: Number\n",
            ),
            (
                "two.bl",
                "namespace orders\nversion \"1\"\ntype Shared:\n  value: Bool\n",
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
                "namespace orders\nversion \"1\"\ntype Shared:\n  value: Number\n",
            ),
            (
                "two.bl",
                "namespace orders\nversion \"2\"\nprocess route(input: Shared) -> Number:\n  node start = start\n  node done = end\n  link start -> done(1)\n",
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
    let process = "namespace orders\nversion \"1\"\nprocess route(input: Order) -> Order:\n  node start = start\n  node value = task echo(input)\n  node done = end\n  link start -> value\n  link value -> done(value)\n";
    let definitions = "namespace orders\nversion \"1\"\ntype Order:\n  value: Number\ntask echo(input: Order) -> Order:\n  return input\n";
    let other = "namespace orders\nversion \"2\"\nprocess other(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\n";
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
        .build()
        .unwrap();
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = \"consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\norders = {{ path = {:?} }}\nserde_json = \"1\"\n", root.join(".blkit").to_str().unwrap())).unwrap();
    fs::write(consumer.join("src/lib.rs"), "#[test] fn imported_processes_run() { let defs = orders::named_graph_definitions(); assert_eq!(defs.len(), 2); let graph = defs.into_iter().find(|d| d.version == \"1\").unwrap(); let input = serde_json::json!({\"value\": \"2\"}); let mut checkpoint = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut checkpoint).unwrap(), input); }\n").unwrap();
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
        .build()
        .unwrap();
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
    let error = updated.build().err().unwrap();
    assert!(
        error.contains("lock") || error.contains("locked"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&lockfile).unwrap(), locked);
    updated.update().unwrap();
    updated.build().unwrap();
    assert!(
        fs::read_to_string(&lockfile)
            .unwrap()
            .contains("name = \"extra\"\nversion = \"0.1.1\"")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_refreshes_lockfile_when_a_referenced_extension_version_changes() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let source = "namespace orders\nversion \"1\"\nprocess route(input: Number) -> Number:\n  node start = start\n  node charge = task payments.charge(input)\n  node done = end\n  link start -> charge\n  link charge -> done(charge)\n";
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
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
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
    let error = updated.build().err().unwrap();
    assert!(
        error.contains("lock") || error.contains("locked"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".blkit/Cargo.lock")).unwrap(),
        locked
    );
    updated.update().unwrap();
    updated.build().unwrap();
    assert!(
        fs::read_to_string(root.join(".blkit/Cargo.lock"))
            .unwrap()
            .contains("name = \"payments\"\nversion = \"0.1.1\"")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_extension_dependency_stops_the_build() {
    let root = project(
        &format!(
            "{MANIFEST}[dependencies]\nmissing = {{ version = \"0.1.0\", path = \"not-there\" }}\n"
        ),
        &[("source.bl", SOURCE)],
    );
    let error = blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .err()
        .unwrap();
    assert!(
        error.contains("missing") && error.contains("not-there"),
        "{error}"
    );
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
        .build()
        .unwrap();
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
    project.build().unwrap();
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
        project.build().unwrap();
        let error = project.extensions().err().unwrap();
        assert!(
            error.contains("charge") || error.contains("List<"),
            "{label}: {error}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn crate_qualified_tasks_resolve_across_files_and_check_types() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let types = "namespace orders\nversion \"1\"\ntype Order:\n  amount: Number\ntype Receipt:\n  id: String\n";
    let process = "namespace orders\nversion \"1\"\nprocess charge_order(input: Order) -> Receipt:\n  node start = start\n  node paid = task payments.charge(input)\n  node done = end\n  link start -> paid\n  link paid -> done(paid)\n";
    let descriptor = "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n";
    let root = project(
        &manifest,
        &[
            ("a-process.bl", process),
            ("z-types.bl", types),
            (
                "payments/Cargo.toml",
                "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
            ("payments/src/lib.rs", "pub fn placeholder() {}"),
            ("payments/blkit-tasks.toml", descriptor),
        ],
    );
    let project = blkit::project::Project::load(&root).unwrap();
    assert_eq!(project.programs().unwrap()[0].processes.len(), 1);
    fs::write(
        root.join("a-process.bl"),
        process.replace(
            "task payments.charge(input)",
            "task payments.missing(input)",
        ),
    )
    .unwrap();
    assert!(
        project
            .programs()
            .err()
            .unwrap()
            .contains("payments.missing")
    );
    fs::write(
        root.join("a-process.bl"),
        process.replace("task payments.charge(input)", "task unknown.charge(input)"),
    )
    .unwrap();
    assert!(project.programs().err().unwrap().contains("unknown.charge"));
    fs::write(
        root.join("a-process.bl"),
        process.replace(
            "task payments.charge(input)",
            "task payments.charge(input.amount)",
        ),
    )
    .unwrap();
    assert!(
        project
            .programs()
            .err()
            .unwrap()
            .contains("task input type mismatch")
    );
    fs::write(
        root.join("a-process.bl"),
        process.replace("done(paid)", "done(input)"),
    )
    .unwrap();
    assert!(project.programs().err().unwrap().contains("type"));
    fs::write(root.join("a-process.bl"), process).unwrap();
    fs::write(
        root.join("payments/blkit-tasks.toml"),
        descriptor.replace("input = \"Order\"", "input = \"Unknown\""),
    )
    .unwrap();
    assert!(project.programs().err().unwrap().contains("Unknown"));
    fs::write(
        root.join("a-process.bl"),
        process.replace("payments.charge", "payments..charge"),
    )
    .unwrap();
    assert!(
        project
            .programs()
            .err()
            .unwrap()
            .contains("invalid task node")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_build_links_async_extension_and_rejects_wrong_callable() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let source = "namespace orders\nversion \"1\"\ntype Order:\n  amount: Number\ntype Receipt:\n  id: String\nprocess charge_order(input: Order) -> Receipt:\n  node start = start\n  node paid = task payments.charge(input)\n  node done = end\n  link start -> paid\n  link paid -> done(paid)\n";
    let extension_manifest = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde_json = \"1\"\n";
    let good = "pub async fn charge(_input: serde_json::Value) -> Result<serde_json::Value, String> { Ok(serde_json::json!({\"id\": \"ok\"})) }\n";
    let descriptor = "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n";
    let root = project(
        &manifest,
        &[
            ("source.bl", source),
            ("payments/Cargo.toml", extension_manifest),
            ("payments/src/lib.rs", good),
            ("payments/blkit-tasks.toml", descriptor),
        ],
    );
    let project = blkit::project::Project::load(&root).unwrap();
    project.build().unwrap();
    fs::write(root.join("payments/src/lib.rs"), "pub fn charge() {}\n").unwrap();
    let error = project.build().err().unwrap();
    assert!(
        error.contains("charge") && error.contains("cargo build failed"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn qualified_each_and_loop_calls_execute_async_extension_tasks() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let source = "namespace orders\nversion \"1\"\nprocess gather(input: List<Number>) -> List<Number>:\n  node start = start\n  node batch = task payments.increment each input parallel\n  node done = end\n  link start -> batch\n  link batch -> done(batch)\nprocess gather_sequential(input: List<Number>) -> List<Number>:\n  node start = start\n  node batch = task payments.increment each input sequential\n  node done = end\n  link start -> batch\n  link batch -> done(batch)\nprocess repeat(input: Number) -> Number:\n  node start = start\n  node repeated = task payments.increment(repeated) repeat_pre(repeated < 3) max_iterations 3 initial input\n  node done = end\n  link start -> repeated\n  link repeated -> done(repeated)\nprocess once(input: Number) -> Number:\n  node start = start\n  node repeated = task payments.increment(input) repeat_post(repeated < 0) max_iterations 3\n  node done = end\n  link start -> repeated\n  link repeated -> done(repeated)\nprocess runaway(input: Number) -> Number:\n  node start = start\n  node repeated = task payments.increment(input) repeat_post(repeated < 99) max_iterations 2\n  node done = end\n  link start -> repeated\n  link repeated -> done(repeated)\n";
    let extension_manifest = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde_json = \"1\"\n";
    let function = "pub async fn increment(input: serde_json::Value) -> Result<serde_json::Value, String> { let value: u32 = input.as_str().ok_or(\"not a string\")?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?; Ok(serde_json::json!((value + 1).to_string())) }\n";
    let descriptor = "[[tasks]]\nname = \"increment\"\nfunction = \"increment\"\ninput = \"Number\"\noutput = \"Number\"\n";
    let root = project(
        &manifest,
        &[
            ("source.bl", source),
            ("payments/Cargo.toml", extension_manifest),
            ("payments/src/lib.rs", function),
            ("payments/blkit-tasks.toml", descriptor),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
    let generated = root.join(".blkit");
    fs::create_dir_all(generated.join("tests")).unwrap();
    fs::write(generated.join("tests/custom.rs"), r#"
#[tokio::test]
async fn qualified_each_and_loops_execute() {
    use std::time::Duration;
    let path = std::env::temp_dir().join(format!("orders-each-loop-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = blkit::runtime::Store::open(&path).await.unwrap();
    for graph in orders::named_graph_definitions() {
        let input = if graph.name.starts_with("gather") { serde_json::json!(["1"]) } else { serde_json::json!("1") };
        let mut checkpoint = graph.checkpoint(&input).unwrap();
        assert!(graph.run(&input, &mut checkpoint).unwrap_err().contains("async task requires async executor"));
    }
    let engine = blkit::runtime::Engine::new(blkit::runtime::Registry::new_named(orders::named_graph_definitions()).unwrap(), store.clone(), 2).unwrap();
    for (name, input, expected) in [
        ("gather", serde_json::json!(["1", "2"]), serde_json::json!(["2", "3"])),
        ("gather_sequential", serde_json::json!(["1", "2"]), serde_json::json!(["2", "3"])),
        ("repeat", serde_json::json!("1"), serde_json::json!("3")),
        ("once", serde_json::json!("1"), serde_json::json!("2")),
    ] {
        let id = engine.start("orders", "1", name, input).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let item = store.get(&id).await.unwrap().unwrap();
                if item.status == "completed" { break item; }
                assert_ne!(item.status, "failed", "{name}: {:?}", item.error);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        assert_eq!(result.result, Some(expected), "{name}");
    }
    let id = engine.start("orders", "1", "runaway", serde_json::json!("1")).await.unwrap();
    let stopped = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let item = store.get(&id).await.unwrap().unwrap();
            if item.status == "business-error" { break item; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    assert_eq!(stopped.terminal_name.as_deref(), Some("task-iteration-limit"));
    assert!(stopped.result.is_none());
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
"#).unwrap();
    let result = std::process::Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(generated.join("Cargo.toml"))
        .args(["--test", "custom"])
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
fn generated_async_task_retries_invalid_output_without_committing_it() {
    let manifest = format!(
        "{MANIFEST}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n"
    );
    let source = "namespace orders\nversion \"1\"\ntype Order:\n  amount: Number\ntype Receipt:\n  id: String\nprocess charge_order(input: Order) -> Receipt:\n  retry max_retries 1 retry_for \"10s\" retry_delay \"300ms\" backoff exponential\n  node start = start\n  node paid = task payments.charge(input)\n  node done = end\n  link start -> paid\n  link paid -> done(paid)\n";
    let extension_manifest = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde_json = \"1\"\n";
    let task = "use std::sync::atomic::{AtomicUsize, Ordering};\npub static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);\npub async fn charge(_input: serde_json::Value) -> Result<serde_json::Value, String> { if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 { Ok(serde_json::json!({\"id\": 42})) } else { Ok(serde_json::json!({\"id\": \"ok\"})) } }\n";
    let descriptor = "[[tasks]]\nname = \"charge\"\nfunction = \"charge\"\ninput = \"Order\"\noutput = \"Receipt\"\n";
    let root = project(
        &manifest,
        &[
            ("source.bl", source),
            ("payments/Cargo.toml", extension_manifest),
            ("payments/src/lib.rs", task),
            ("payments/blkit-tasks.toml", descriptor),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
    let generated = root.join(".blkit");
    let cargo = generated.join("Cargo.toml");
    let mut text = fs::read_to_string(&cargo).unwrap();
    text.push_str("[dev-dependencies]\ntokio = { version = \"1\", features = [\"macros\", \"rt\", \"time\"] }\n");
    fs::write(&cargo, text).unwrap();
    fs::create_dir_all(generated.join("tests")).unwrap();
    fs::write(generated.join("tests/custom.rs"), r#"
#[tokio::test]
async fn invalid_output_is_not_committed_and_is_retried() {
    use std::{sync::atomic::Ordering, time::Duration};
    let path = std::env::temp_dir().join(format!("orders-custom-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = blkit::runtime::Store::open(&path).await.unwrap();
    let engine = blkit::runtime::Engine::new(
        blkit::runtime::Registry::new_named(orders::named_graph_definitions()).unwrap(), store.clone(), 2
    ).unwrap();
    let id = engine.start("orders", "1", "charge_order", serde_json::json!({"amount":"5"})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.get(&id).await.unwrap().unwrap().status != "retry-waiting" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    let waiting = store.get(&id).await.unwrap().unwrap();
    assert!(waiting.checkpoint.unwrap().completed.get("paid").is_none());
    assert_eq!(waiting.attempt, 1);
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.get(&id).await.unwrap().unwrap().status != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    assert_eq!(payments::ATTEMPTS.load(Ordering::SeqCst), 2);
    assert_eq!(store.get(&id).await.unwrap().unwrap().result, Some(serde_json::json!({"id":"ok"})));
    drop(engine);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
"#).unwrap();
    let result = std::process::Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(&cargo)
        .args(["--test", "custom"])
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
fn switching_project_target_keeps_lockfile_and_only_selected_binary() {
    let source = "namespace orders\nversion \"1\"\nprocess route(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\n";
    let root = project(MANIFEST, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
    let locked = fs::read(root.join(".blkit/Cargo.lock")).unwrap();
    fs::write(
        root.join("blkit.toml"),
        MANIFEST.replace("\"crate\"", "\"worker\""),
    )
    .unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
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
        .build()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-server.rs").exists());
    assert!(!root.join(".blkit/src/bin/orders-worker.rs").exists());
    assert_eq!(fs::read(root.join(".blkit/Cargo.lock")).unwrap(), locked);
    fs::write(root.join("blkit.toml"), MANIFEST).unwrap();
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
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
    let source = "namespace orders\nversion \"1\"\nprocess route(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\n";
    let manifest = MANIFEST.replace("\"crate\"", "\"worker\"");
    let root = project(&manifest, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
    assert!(root.join(".blkit/src/bin/orders-worker.rs").exists());
    fs::write(
        root.join("blkit.toml"),
        manifest.replace("name = \"orders\"", "name = \"purchases\""),
    )
    .unwrap();
    let renamed = blkit::project::Project::load(&root).unwrap();
    renamed.update().unwrap();
    renamed.build().unwrap();
    assert!(root.join(".blkit/src/bin/purchases-worker.rs").exists());
    assert!(!root.join(".blkit/src/bin/orders-worker.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_server_executes_compiled_process_over_loopback_rest() {
    use std::{
        net::{TcpListener, TcpStream},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let source = "namespace orders\nversion \"1\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node work = task echo(input)\n  node done = end\n  link start -> work\n  link work -> done(work)\n";
    let root = project(
        &MANIFEST.replace("\"crate\"", "\"server\""),
        &[("route.bl", source)],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
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
        .stdout(Stdio::null())
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
    let id = http_request(port, "POST", "/processes/orders/1/route/instances", "\"7\"")["id"]
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
    child.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_server_restarts_async_task_from_last_committed_checkpoint() {
    use std::{
        net::{TcpListener, TcpStream},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let manifest = format!(
        "{}[dependencies]\npayments = {{ version = \"0.1.0\", path = \"payments\" }}\n",
        MANIFEST.replace("\"crate\"", "\"server\"")
    );
    let source = "namespace orders\nversion \"1\"\ntype Order:\n  amount: Number\ntype Receipt:\n  id: String\nprocess charge_order(input: Order) -> Receipt:\n  retry max_retries 1 retry_for \"10s\" retry_delay \"10ms\" backoff exponential\n  node start = start\n  node first = task payments.first(input)\n  node second = task payments.second(first)\n  node done = end\n  link start -> first\n  link first -> second\n  link second -> done(second)\nprocess invalid_output(input: Order) -> Receipt:\n  node start = start\n  node bad = task payments.bad(input)\n  node done = end\n  link start -> bad\n  link bad -> done(bad)\n";
    let extension_manifest = "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde_json = \"1\"\ntokio = { version = \"1\", features = [\"time\"] }\n";
    let descriptor = "[[tasks]]\nname = \"first\"\nfunction = \"first\"\ninput = \"Order\"\noutput = \"Receipt\"\n[[tasks]]\nname = \"second\"\nfunction = \"second\"\ninput = \"Receipt\"\noutput = \"Receipt\"\n[[tasks]]\nname = \"bad\"\nfunction = \"bad\"\ninput = \"Order\"\noutput = \"Receipt\"\n";
    let functions = r#"use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write};
fn effect(name: &str) {
    writeln!(OpenOptions::new().create(true).append(true).open(std::env::var("BLKIT_TASK_LOG").unwrap()).unwrap(), "{name}").unwrap();
}
pub async fn first(_: Value) -> Result<Value, String> { effect("first"); Ok(json!({"id":"ok"})) }
pub async fn second(input: Value) -> Result<Value, String> {
    effect("second");
    while !std::path::Path::new(&std::env::var("BLKIT_TASK_RELEASE").unwrap()).exists() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Ok(input)
}
pub async fn bad(_: Value) -> Result<Value, String> { Ok(json!({"id":42})) }
"#;
    let root = project(
        &manifest,
        &[
            ("route.bl", source),
            ("payments/Cargo.toml", extension_manifest),
            ("payments/src/lib.rs", functions),
            ("payments/blkit-tasks.toml", descriptor),
        ],
    );
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
    let binary = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join(".blkit/target/debug/orders-server"),
        |target| PathBuf::from(target).join("debug/orders-server"),
    );
    let database = root.join("local.db");
    let log = root.join("effects.log");
    let release = root.join("release");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let start = || {
        Command::new(&binary)
            .arg(&database)
            .arg("2")
            .arg(format!("127.0.0.1:{port}"))
            .env("BLKIT_TASK_LOG", &log)
            .env("BLKIT_TASK_RELEASE", &release)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut child = start();
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    }
    let input = r#"{"amount":"5"}"#;
    let id = http_request(
        port,
        "POST",
        "/processes/orders/1/charge_order/instances",
        input,
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    loop {
        let item = http_request(port, "GET", &format!("/instances/{id}"), "");
        if item["checkpoint"]["completed"]["first"].is_object()
            && fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("second")
        {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "{item}");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    fs::write(&release, "ready").unwrap();
    let mut child = start();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(8));
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = loop {
        let item = http_request(port, "GET", &format!("/instances/{id}"), "");
        if item["status"] == "completed" {
            break item;
        }
        assert!(started.elapsed() < Duration::from_secs(8), "{item}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(result["result"], serde_json::json!({"id":"ok"}));
    let effects = fs::read_to_string(&log).unwrap();
    assert_eq!(
        effects.lines().filter(|line| *line == "first").count(),
        1,
        "{effects}"
    );
    assert_eq!(
        effects.lines().filter(|line| *line == "second").count(),
        2,
        "{effects}"
    );
    let bad = http_request(
        port,
        "POST",
        "/processes/orders/1/invalid_output/instances",
        input,
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    loop {
        let item = http_request(port, "GET", &format!("/instances/{bad}"), "");
        if item["status"] == "failed" {
            assert!(
                item["error"].as_str().unwrap().contains("invalid type"),
                "{item}"
            );
            assert!(item["result"].is_null());
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(8), "{item}");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_worker_binary_claims_only_its_compiled_process_version() {
    use blkit::{
        named_runtime::{GraphDefinition, GraphLink, GraphNode, GraphNodeKind},
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
    let source = "namespace orders\nversion \"1\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node work = task echo(input)\n  node done = end\n  link start -> work\n  link work -> done(work)\n";
    let root = project(&manifest, &[("route.bl", source)]);
    blkit::project::Project::load(&root)
        .unwrap()
        .build()
        .unwrap();
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
                name: "work",
                kind: GraphNodeKind::Task(Arc::new(|input, _| Ok(input.clone()))),
            },
            GraphNode {
                name: "done",
                kind: GraphNodeKind::End,
            },
        ],
        links: vec![
            GraphLink {
                source: "start",
                target: "work",
                value: None,
                condition: None,
                fallback: false,
                label: None,
            },
            GraphLink {
                source: "work",
                target: "done",
                value: Some(Arc::new(|_, values| Ok(values["work"].clone()))),
                condition: None,
                fallback: false,
                label: None,
            },
        ],
    };
    for (id, version) in [("matched", "1"), ("other-version", "2")] {
        let mut item = Instance::new(id, "orders", version, "route", json!("7"));
        item.checkpoint = Some(graph.checkpoint(&item.input).unwrap());
        store.create(&item).await.unwrap();
    }
    let mut child = Command::new(binary)
        .arg(&url)
        .args(["32", "5000"])
        .stdout(Stdio::null())
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
    child.wait().unwrap();
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
