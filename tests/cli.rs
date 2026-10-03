use std::process::Command;

#[test]
fn help_explains_source_and_output() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("SOURCE.bl"));
    assert!(text.contains("OUTPUT.rs"));
    assert!(text.contains("build"));
    assert!(text.contains("update"));
}

#[test]
fn subcommand_help_and_version_do_not_run_projects() {
    for command in [["build", "--help"], ["update", "--help"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(command)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("PROJECT_DIR"));
    }
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("0.1.0"));
}

#[test]
fn bash_completion_is_printed_without_a_project() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["--completions", "bash"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let script = String::from_utf8(result.stdout).unwrap();
    assert!(script.contains("build") && script.contains("update"));
    assert!(script.contains("--completions"));
    assert!(result.stderr.is_empty());
}

#[test]
fn invalid_cli_combinations_fail_before_writing_output() {
    for args in [
        vec!["missing.bl"],
        vec!["missing.bl", "output.rs", "build"],
        vec!["build", "project", "extra"],
        vec!["--completions", "no-such-shell"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{args:?}");
        assert!(result.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("Usage:") || stderr.contains("For more information, try '--help'."),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn project_commands_default_to_current_directory() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-default-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for command in ["build", "update"] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg(command)
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("blkit.toml"));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn missing_arguments_fail_with_usage() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit")).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr).unwrap().contains("Usage:"));
}

#[test]
fn valid_source_writes_rust() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-valid-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace orders\nversion \"1.0\"\nprocess echo(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args([&input, &output])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        std::fs::read_to_string(&output)
            .unwrap()
            .contains("pub fn named_graph_definitions")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn build_command_reports_invalid_project_manifest() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-project-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("blkit.toml"),
        "[project]\nname = \"demo\"\nblkit = \"0.1.0\"\nbuild_target = \"invalid\"\n",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("build")
        .arg(&directory)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("build_target")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn update_command_reports_invalid_project_manifest() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-update-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("blkit.toml"),
        "[project]\nname = \"demo\"\nblkit = \"0.1.0\"\nbuild_target = \"invalid\"\n",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("update")
        .arg(&directory)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("build_target")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_source_produces_diagnostic_without_output() {
    for (case, source, error) in [
        ("parse", "namespace orders\n", "version"),
        (
            "type",
            "namespace orders\nversion \"1.0\"\nprocess echo(input: Unknown) -> Number:\n  node start = start\n  node done = end\n  link start -> done(1)\n",
            "Unknown",
        ),
    ] {
        let directory =
            std::env::temp_dir().join(format!("blkit-cli-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let input = directory.join("input.bl");
        let output = directory.join("output.rs");
        std::fs::write(&input, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args([&input, &output])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(error));
        assert!(!output.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn errors_include_source_and_project_context_without_terminal_controls() {
    let directory =
        std::env::temp_dir().join(format!("blkit-cli-diagnostics-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("invalid.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace orders\n").unwrap();
    std::fs::write(
        directory.join("blkit.toml"),
        "[project]\nname = \"demo\"\nblkit = \"0.1.0\"\nbuild_target = \"invalid\"\n",
    )
    .unwrap();
    for (arguments, context, detail) in [
        (
            vec![input.as_os_str(), output.as_os_str()],
            input.to_string_lossy().to_string(),
            "version",
        ),
        (
            vec![std::ffi::OsStr::new("build"), directory.as_os_str()],
            directory.to_string_lossy().to_string(),
            "build_target",
        ),
        (
            vec![std::ffi::OsStr::new("update"), directory.as_os_str()],
            directory.to_string_lossy().to_string(),
            "build_target",
        ),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(arguments)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(
            stderr.contains(&context) && stderr.contains(detail),
            "{stderr}"
        );
        assert!(
            !stderr.contains('\u{1b}') && !stderr.contains('\r'),
            "{stderr:?}"
        );
        assert!(
            !stderr.contains("Diagnostic {") && !stderr.contains("NOTE:"),
            "{stderr}"
        );
    }
    assert!(!output.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn clicolor_zero_disables_interactive_styling_and_progress() {
    // `script` provides a real pseudo-terminal without adding a test dependency.
    if Command::new("script").arg("--version").output().is_err() {
        return;
    }
    let directory = std::env::temp_dir().join(format!("blkit-cli-clicolor-{}", std::process::id()));
    let command = format!(
        "{} build {}",
        env!("CARGO_BIN_EXE_blkit"),
        directory.display()
    );
    let result = Command::new("script")
        .args(["-q", "-e", "-c", &command, "/dev/null"])
        .env("CLICOLOR", "0")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(text.contains(&*directory.to_string_lossy()));
    assert!(!text.contains('\u{1b}'), "{text:?}");
}
