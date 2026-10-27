use std::{fs, path::PathBuf, process::Command};

struct DiscoveryDirectory(PathBuf);

impl DiscoveryDirectory {
    fn new(case: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("blkit-cli-discovery-{case}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("work")).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            for tool in ["git", "cargo", "rustc"] {
                let executable = path.join("bin").join(tool);
                fs::write(
                    &executable,
                    "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$BLKIT_TEST_TOOL_LOG\"\nexit 99\n",
                )
                .unwrap();
                fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
        command
            .current_dir(self.0.join("work"))
            .env("PATH", self.0.join("bin"))
            .env("BLKIT_TEST_TOOL_LOG", self.0.join("tool-calls"));
        command
    }

    fn assert_untouched(&self) {
        assert_eq!(fs::read_dir(self.0.join("work")).unwrap().count(), 0);
        let mut entries: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        entries.sort();
        assert_eq!(entries, ["bin", "work"], "unexpected files or tool calls");
    }
}

impl Drop for DiscoveryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const SOURCE: &str = "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }";

#[test]
fn help_explains_source_output_and_init_without_side_effects() {
    let directory = DiscoveryDirectory::new("help");
    let result = directory.command().arg("--help").output().unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("SOURCE.bl"));
    assert!(text.contains("OUTPUT.rs"));
    assert!(text.contains("transpile"));
    assert!(text.contains("update"));
    assert!(text.contains("init") && text.contains("standalone local project"));
    assert!(text.contains("--no-vcs"));
    assert!(!text.contains("--vcs"));
    assert!(result.stderr.is_empty());
    directory.assert_untouched();
}

#[test]
fn init_help_explains_destination_and_no_vcs_without_side_effects() {
    let directory = DiscoveryDirectory::new("init-help");
    for args in [
        vec!["init", "--help"],
        vec!["init", "new-project", "--help"],
        vec!["init", "new-project", "--no-vcs", "--help"],
    ] {
        let result = directory.command().args(&args).output().unwrap();
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        for requirement in ["PROJECT_DIR", "--no-vcs", "current directory", "Git"] {
            assert!(
                text.contains(requirement),
                "help omits {requirement}: {text}"
            );
        }
        assert!(!text.contains("--vcs"));
        assert!(
            !text.contains("--no-vcs <"),
            "flag must not take a value: {text}"
        );
        assert!(result.stderr.is_empty());
        directory.assert_untouched();
    }
}

#[test]
fn subcommand_help_and_version_do_not_run_projects() {
    let directory = DiscoveryDirectory::new("subcommand-help");
    for command in [["transpile", "--help"], ["update", "--help"]] {
        let result = directory.command().args(command).output().unwrap();
        assert!(
            result.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("PROJECT_DIR"));
    }
    let result = directory.command().arg("--version").output().unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("0.1.0"));
    directory.assert_untouched();
}

#[test]
fn completions_expose_init_and_no_vcs_without_side_effects() {
    let directory = DiscoveryDirectory::new("completions");
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let result = directory
            .command()
            .args(["--completions", shell])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let script = String::from_utf8(result.stdout).unwrap();
        for requirement in ["init", "transpile", "update"] {
            assert!(script.contains(requirement), "{shell} omits {requirement}");
        }
        let flag = if shell == "fish" {
            "-l no-vcs"
        } else {
            "--no-vcs"
        };
        assert!(script.contains(flag), "{shell} omits {flag}");
        let legacy_option = if shell == "fish" { "-l vcs" } else { "--vcs" };
        assert!(
            !script.contains(legacy_option),
            "{shell} retains {legacy_option}"
        );
        if shell == "bash" {
            assert!(!script.contains("_blkit__build"));
            assert!(script.contains("--completions"));
        }
        assert!(result.stderr.is_empty());
        directory.assert_untouched();
    }
}

#[test]
fn invalid_cli_combinations_fail_without_side_effects_or_echoing_arguments() {
    let directory = DiscoveryDirectory::new("invalid");
    let credential = "postgresql://user:init-credential-sentinel@localhost/db";
    for args in [
        vec!["missing.bl"],
        vec!["missing.bl", "output.rs", "transpile"],
        vec!["transpile", "project", "extra"],
        vec!["--completions", "no-such-shell"],
        vec!["init", "new-project", "--vcs", "git"],
        vec!["init", "new-project", "--vcs", "none"],
        vec!["init", "new-project", "--vcs", "hg"],
        vec!["init", "new-project", "--vcs"],
        vec!["init", "new-project", "--no-vcs=none"],
        vec!["init", "--no-vcs=git"],
        vec!["init", "--no-vcs=true"],
        vec!["init", "--no-vcs=false"],
        vec!["init", "new-project", "extra-argument-sentinel"],
        vec!["init", "new-project", "--no-vcs", "extra-argument-sentinel"],
        vec!["init", credential],
        vec!["init", "new-project", "--vcs", credential],
        vec!["init", "new-project", "--no-vcs", credential],
    ] {
        let result = directory.command().args(&args).output().unwrap();
        assert!(!result.status.success(), "{args:?}");
        assert!(result.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("Usage:") || stderr.contains("For more information, try '--help'."),
            "{args:?}: {stderr}"
        );
        assert!(stderr.contains("invalid arguments; use --help"), "{stderr}");
        for secret in ["init-credential-sentinel", "extra-argument-sentinel"] {
            assert!(!stderr.contains(secret), "argument was echoed: {stderr}");
        }
        directory.assert_untouched();
    }
}

#[test]
fn project_commands_default_to_current_directory() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-default-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for command in ["transpile", "update"] {
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
    std::fs::write(&input, SOURCE).unwrap();
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
fn transpile_generates_projects_without_cargo_or_a_binary() {
    for target in ["crate", "worker", "server"] {
        let directory = std::env::temp_dir().join(format!(
            "blkit-cli-transpile-{target}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("blkit.toml"),
            format!(
                "[project]\nname = \"orders\"\nblkit = \"0.1.0\"\nbuild_target = \"{target}\"\n"
            ),
        )
        .unwrap();
        std::fs::write(directory.join("route.bl"), SOURCE).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("transpile")
            .current_dir(&directory)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(directory.join(".blkit/Cargo.toml").exists());
        assert!(directory.join(".blkit/src/lib.rs").exists());
        assert!(!directory.join(".blkit/target").exists());
        if target != "crate" {
            assert!(
                directory
                    .join(format!(".blkit/src/bin/orders-{target}.rs"))
                    .exists()
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn transpile_rejects_legacy_extension_calls_without_cargo() {
    let directory =
        std::env::temp_dir().join(format!("blkit-cli-extension-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("blkit.toml"), "[project]\nname = \"orders\"\nblkit = \"0.1.0\"\nbuild_target = \"crate\"\n[dependencies]\npayments = { version = \"0.1.0\", path = \"payments\" }\n").unwrap();
    std::fs::write(directory.join("route.bl"), "namespace orders; version \"1\"; process route { node work = task payments.charge(input); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("transpile")
        .arg(&directory)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("invalid process statement"), "{stderr}");
    assert!(!directory.join(".blkit/Cargo.toml").exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn removed_build_command_reports_usage() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["build", "/not-a-project"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Usage:"));
}

#[test]
fn transpile_command_reports_invalid_project_manifest() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-project-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("blkit.toml"),
        "[project]\nname = \"demo\"\nblkit = \"0.1.0\"\nbuild_target = \"invalid\"\n",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("transpile")
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
        ("parse", "namespace orders;\n", "version"),
        (
            "type",
            "namespace orders; version \"1\"; start_event start { output input: Unknown; } end_event done { input result: Unknown; } process route { flow start -> done; bind start.input -> done.result; }",
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
fn direct_transpile_reports_original_utf8_source_and_global_error_without_fake_label() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-miette-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.bl");
    let output = directory.join("output.rs");
    for (source, explanation, excerpt, located) in [
        (
            "namespace orders; version \"1\";\n# café\nstart_event start { output amount: Missing; }",
            "unknown type: Missing",
            "start_event start { output amount: Missing; }",
            Some("label at line 3, columns 36 to 42"),
        ),
        (
            "namespace orders; version \"1\";\n# café\n@",
            "invalid character",
            "snippet line 3: @",
            Some("label at line 3, column 1"),
        ),
        (
            "namespace orders; version \"1\";\nprocess route {",
            "unclosed brace",
            "snippet line 2: process route {",
            Some("label at line 2"),
        ),
        (
            "namespace orders; version \"1\";\nstart_event start { output amount: Number; } process route { }",
            "start",
            "snippet line",
            None,
        ),
    ] {
        std::fs::write(&input, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args([&input, &output])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(
            stderr.contains(&*input.to_string_lossy()) && stderr.contains(explanation),
            "{stderr}"
        );
        if let Some(label) = located {
            assert!(
                stderr.contains(excerpt) && stderr.contains(label),
                "{stderr}"
            );
        } else {
            assert!(
                !stderr.contains(excerpt)
                    && !stderr.contains("Begin snippet")
                    && !stderr.contains("label at"),
                "{stderr}"
            );
        }
        assert!(
            !stderr.contains('\u{1b}') && !stderr.contains('\r'),
            "{stderr:?}"
        );
        assert!(!output.exists());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn direct_transpile_labels_invalid_retry_delay_without_output() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-retry-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace demo; version \"1\";\n# café\nprocess route { retry max_retries 1 retry_for \"1s\" retry_delay \"bogus\" backoff exponential; }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args([&input, &output])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains(&*input.to_string_lossy()), "{stderr}");
    assert!(stderr.contains("invalid retry duration: bogus"), "{stderr}");
    assert!(stderr.contains("retry_delay \"bogus\""), "{stderr}");
    assert!(
        stderr.contains("label at line 3, columns 64 to 70"),
        "{stderr}"
    );
    assert!(
        !stderr.contains('\u{1b}') && !stderr.contains('\r'),
        "{stderr:?}"
    );
    assert!(result.stdout.is_empty());
    assert!(!output.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn direct_transpile_reports_both_recovered_source_errors() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-recovery-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace demo; version \"1.0\";\n# café\nstart_event start { output first: ; output second: ; }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args([&input, &output])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains(&*input.to_string_lossy()), "{stderr}");
    assert!(
        stderr.contains("label at line 3, column 35")
            && stderr.contains("label at line 3, column 52"),
        "{stderr}"
    );
    assert!(
        !stderr.contains('\u{1b}') && !stderr.contains('\r'),
        "{stderr:?}"
    );
    assert!(!output.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn errors_include_source_and_project_context_without_terminal_controls() {
    let directory =
        std::env::temp_dir().join(format!("blkit-cli-diagnostics-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("invalid.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace orders;\n").unwrap();
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
            vec![std::ffi::OsStr::new("transpile"), directory.as_os_str()],
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
        "{} transpile {}",
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
