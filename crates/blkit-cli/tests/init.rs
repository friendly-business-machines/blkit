use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use blkit_transpiler::{Type, graph::PeerKind, project::Project};

struct InitDirectory(PathBuf);

impl InitDirectory {
    fn new(case: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("blkit-cli-init-{case}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for InitDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_init_failure(result: &Output, diagnostic: &str) {
    assert!(!result.status.success(), "init unexpectedly succeeded");
    assert!(result.stdout.is_empty(), "failure printed success output");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains(diagnostic), "{stderr}");
    assert!(!stderr.contains("warning:"), "{stderr}");
}

fn assert_no_new_output(directory: &Path, existing: &str) {
    for filename in ["blkit.toml", "main.bl", ".git"] {
        if filename != existing {
            assert!(
                directory.join(filename).symlink_metadata().is_err(),
                "unexpected output: {filename}"
            );
        }
    }
}

fn assert_scaffold(directory: &Path, name: &str) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    entries.sort();
    // Only authoring inputs: no root Cargo files, Git, generated code, or runtime data.
    assert_eq!(entries, ["blkit.toml", "main.bl"]);
    assert_eq!(
        fs::read_to_string(directory.join("blkit.toml")).unwrap(),
        format!(
            "[project]\nname = \"{name}\"\nblkit = \"{}\"\nbuild_target = \"api-worker\"\npersistence = \"local\"\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    let project = Project::load(directory).unwrap();
    assert!(project.dependencies.is_empty());
    assert_eq!(project.sources, [project.root.join("main.bl")]);

    let source = fs::read_to_string(directory.join("main.bl")).unwrap();
    let program = blkit_transpiler::parse(&source).unwrap();
    assert_eq!(program.namespace, "example");
    assert_eq!(program.version, "1");
    assert_eq!(program.processes.len(), 1);
    assert_eq!(program.processes[0].name, "echo");
    assert!(program.tasks.is_empty());
    assert!(program.external_tasks.is_empty());
    assert_eq!(program.peer_nodes.len(), 2);
    assert!(program.peer_nodes.iter().any(|peer| matches!(
        &peer.kind,
        PeerKind::Start { outputs }
            if outputs.len() == 1 && outputs[0].1 == Type::Named("Number".into())
    )));
    assert!(program.peer_nodes.iter().any(|peer| matches!(
        &peer.kind,
        PeerKind::End { inputs }
            if inputs.len() == 1 && inputs[0].1 == Type::Named("Number".into())
    )));
    // Validate and generate in memory without creating transpilation artifacts.
    blkit_transpiler::transpile(&source).unwrap();
}

fn git_fixture_command(directory: &Path, executable: &str) -> Command {
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", directory.join("absent-global-config"))
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_0", "fixture-main")
        .env("GIT_CONFIG_KEY_1", "init.templateDir")
        .env("GIT_CONFIG_VALUE_1", "")
        .current_dir(directory);
    command
}

fn fixture_git(directory: &Path, arguments: &[&str]) -> Output {
    let output = git_fixture_command(directory, "git")
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn metadata_snapshot(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    collect(directory, directory, &mut files);
    files.sort();
    files
}

#[cfg(unix)]
fn fake_git(parent: &Path, body: &str) -> Command {
    use std::os::unix::fs::PermissionsExt;

    let bin = parent.join("bin");
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("git");
    fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BLKIT_TEST_GIT_LOG\"\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = git_fixture_command(parent, env!("CARGO_BIN_EXE_blkit"));
    command
        .env("PATH", bin)
        .env("BLKIT_TEST_GIT_LOG", parent.join("git-calls"));
    command
}

fn assert_tool_warnings(output: &Output, tools: &[&str]) {
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), tools.len(), "{stderr}");
    for tool in tools {
        let warning = stderr
            .lines()
            .find(|line| line.starts_with(&format!("warning: {tool} ")))
            .unwrap_or_else(|| panic!("missing {tool} warning: {stderr}"));
        for explanation in ["build", "author", "transpile"] {
            assert!(warning.contains(explanation), "{warning}");
        }
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialized project at"), "{stdout}");
    for claim in ["verified", "compatible", "build ready"] {
        assert!(!stdout.contains(claim), "{stdout}");
    }
}

fn guidance_commands(output: &Output) -> Vec<&str> {
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .collect()
}

fn assert_guidance(output: &Output, name: &str) {
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for explanation in [
        "not been transpiled, built, or started",
        "do not verify compiler compatibility or build readiness",
        "CARGO_TARGET_DIR",
        "debug directory instead",
        "127.0.0.1:3000",
        "loopback",
        "no native authentication or TLS",
        "per-instance authorization",
    ] {
        assert!(
            stdout.contains(explanation),
            "missing {explanation}: {stdout}"
        );
    }
    let commands = guidance_commands(output);
    assert!(
        commands.ends_with(&[
            "blkit transpile",
            "cargo build --manifest-path .blkit/Cargo.toml",
            &format!(".blkit/target/debug/{name}-api-worker"),
        ]),
        "{commands:?}"
    );
}

#[test]
fn init_guidance_current_directory_needs_no_enter_directory_step() {
    let parent = InitDirectory::new("guidance-current");
    for (name, destination) in [
        ("omitted", None),
        ("dot", Some(".")),
        ("absolute", Some("absolute")),
    ] {
        let directory = parent.0.join(name);
        fs::create_dir(&directory).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
        command.arg("init").arg("--no-vcs");
        match destination {
            Some("absolute") => {
                command.arg(&directory);
            }
            Some(path) => {
                command.arg(path);
            }
            None => {}
        }
        let output = command
            .current_dir(&directory)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_guidance(&output, name);
        assert_eq!(guidance_commands(&output).len(), 3);
        assert_scaffold(&directory, name);
    }
}

#[test]
fn init_guidance_named_destination_enters_project_before_transpile_build_run() {
    let parent = InitDirectory::new("guidance-named");
    for relative in [true, false] {
        let name = if relative { "my-app" } else { "other-app" };
        let directory = parent.0.join("nested").join(name);
        let destination = if relative {
            PathBuf::from("nested/my-app/")
        } else {
            directory.clone()
        };
        let output = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(destination)
            .arg("--no-vcs")
            .current_dir(&parent.0)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_guidance(&output, name);
        let commands = guidance_commands(&output);
        assert_eq!(commands.len(), 4, "{commands:?}");
        assert_eq!(
            commands[0],
            format!("cd '{}'", directory.canonicalize().unwrap().display())
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(directory.to_str().unwrap()));
        assert_scaffold(&directory, name);
    }
}

#[cfg(unix)]
#[test]
fn init_guidance_cd_preserves_spaces_quotes_and_shell_metacharacters_in_ancestors() {
    use std::os::unix::ffi::OsStrExt;

    let parent = InitDirectory::new("guidance-shell-path");
    let destination = Path::new("space 'quote' \"double\" ; $(printf injected > marker) `printf injected > marker` $HOME * [x] \\").join("my-app");
    let directory = parent.0.join(&destination);
    let output = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(destination)
        .arg("--no-vcs")
        .current_dir(&parent.0)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_guidance(&output, "my-app");
    let commands = guidance_commands(&output);
    assert_eq!(commands.len(), 4, "{commands:?}");
    assert!(commands[0].starts_with("cd "), "{commands:?}");
    let entered = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("{}\nprintf '%s\\0' \"$PWD\"", commands[0]))
        .current_dir(&parent.0)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(entered.status.success(), "{entered:?}");
    let mut expected = directory
        .canonicalize()
        .unwrap()
        .as_os_str()
        .as_bytes()
        .to_vec();
    expected.push(0);
    assert_eq!(entered.stdout, expected);
    assert!(entered.stderr.is_empty(), "{entered:?}");
    assert!(!parent.0.join("marker").exists());
    assert_scaffold(&directory, "my-app");
}

#[cfg(unix)]
#[test]
fn init_guidance_non_utf8_or_control_ancestors_use_escaped_identity_and_manual_entry() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let parent = InitDirectory::new("guidance-manual-entry");
    for ancestor in [
        b"non-utf8-\xff".as_slice(),
        b"line\nbreak",
        b"tab\treturn\r",
        b"escape-\x1b[31m",
    ] {
        let directory = parent.0.join(OsStr::from_bytes(ancestor)).join("my-app");
        let output = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .arg("--no-vcs")
            .current_dir(&parent.0)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_guidance(&output, "my-app");
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert_eq!(
            stdout.lines().next().unwrap(),
            format!(
                "Initialized project at {:?}.",
                directory.canonicalize().unwrap()
            )
        );
        assert!(
            stdout.contains("Enter the destination directory manually"),
            "{stdout}"
        );
        assert_eq!(guidance_commands(&output).len(), 3);
        assert!(
            !stdout.contains(['\x1b', '\t', '\r', '\u{fffd}']),
            "{stdout:?}"
        );
        assert_scaffold(&directory, "my-app");
    }
}

#[test]
fn init_guidance_target_directory_caveat_does_not_interpolate_environment_values() {
    let parent = InitDirectory::new("guidance-target-dir");
    let output = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "my-app", "--no-vcs"])
        .current_dir(&parent.0)
        .env("PATH", "")
        .env(
            "CARGO_TARGET_DIR",
            "untrusted-target ' ; $(printf injected)\n\x1b[31m",
        )
        .output()
        .unwrap();
    assert_guidance(&output, "my-app");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("default target directory"), "{stdout}");
    assert!(!stdout.contains("untrusted-target"), "{stdout}");
    assert!(!stdout.contains('\x1b'), "{stdout}");
    assert_scaffold(&parent.0.join("my-app"), "my-app");
}

#[test]
fn init_warnings_are_plain_when_redirected_or_color_disabled() {
    let parent = InitDirectory::new("plain-warnings");
    for (name, color) in [
        ("redirected", None),
        ("no-color", Some(("NO_COLOR", "1"))),
        ("clicolor", Some(("CLICOLOR", "0"))),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
        command
            .args(["init", name, "--no-vcs"])
            .current_dir(&parent.0)
            .env("PATH", "")
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR");
        if let Some((key, value)) = color {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert_tool_warnings(&output, &["cargo", "rustc"]);
        for stream in [&output.stdout, &output.stderr] {
            assert!(
                stream
                    .iter()
                    .all(|byte| !byte.is_ascii_control() || *byte == b'\n'),
                "{stream:?}"
            );
        }
        assert_scaffold(&parent.0.join(name), name);
    }
}

#[cfg(unix)]
fn fake_rust_tool(parent: &Path, tool: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    let bin = parent.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join(tool);
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\n\
             printf '{tool}|%s|%s|%s|%s\\n' \"$#\" \"$*\" \"$RUSTUP_AUTO_INSTALL\" \"$PWD\" >> \"$BLKIT_TEST_TOOL_LOG\"\n\
             echo untrusted-tool-output\necho untrusted-tool-diagnostic >&2\n\
             [ -f blkit.toml ] && [ -f main.bl ] || exit 91\n\
             {body}\n"
        ),
    )
    .unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn init_probes_rust_tools_separately_after_required_setup_with_fixed_arguments_and_env() {
    let parent = InitDirectory::new("rust-probes");
    let mut command = fake_git(&parent.0, "[ \"$1\" = --version ] || /bin/mkdir .git");
    for (case, cargo_status, rustc_status, warnings) in [
        ("cargo-fails", 71, 0, vec!["cargo"]),
        ("rustc-fails", 0, 72, vec!["rustc"]),
        ("both-succeed", 0, 0, vec![]),
    ] {
        let directory = parent.0.join(case);
        let log = parent.0.join(format!("{case}-calls"));
        for (tool, status) in [("cargo", cargo_status), ("rustc", rustc_status)] {
            fake_rust_tool(
                &parent.0,
                tool,
                &format!("[ -d .git ] || exit 92\nexit {status}"),
            );
        }
        let output = command
            .arg("init")
            .arg(&directory)
            .env("RUSTUP_AUTO_INSTALL", "1")
            .env("BLKIT_TEST_TOOL_LOG", &log)
            .output()
            .unwrap();
        assert_tool_warnings(&output, &warnings);
        assert_guidance(&output, case);
        assert!(directory.join(".git").is_dir());
        Project::load(&directory).unwrap();
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            format!(
                "cargo|1|--version|0|{0}\nrustc|1|--version|0|{0}\n",
                directory.display()
            )
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("untrusted-tool"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("untrusted-tool"));
        command = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"));
        command
            .env("PATH", parent.0.join("bin"))
            .env("BLKIT_TEST_GIT_LOG", parent.0.join("git-calls"));
    }
}

#[cfg(unix)]
#[test]
fn init_nonexecutable_rust_tools_warn_without_failing_setup() {
    use std::os::unix::fs::PermissionsExt;

    let parent = InitDirectory::new("nonexecutable-rust");
    for tool in ["cargo", "rustc"] {
        fake_rust_tool(&parent.0, tool, "exit 0");
        fs::set_permissions(
            parent.0.join("bin").join(tool),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
    let directory = parent.0.join("my-app");
    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(&directory)
        .arg("--no-vcs")
        .env("PATH", parent.0.join("bin"))
        .output()
        .unwrap();
    assert_tool_warnings(&output, &["cargo", "rustc"]);
    assert_scaffold(&directory, "my-app");
}

#[cfg(unix)]
#[test]
fn init_stalled_rust_probe_is_bounded_killed_and_reaped_and_other_probe_runs() {
    use std::time::{Duration, Instant};

    let parent = InitDirectory::new("stalled-rust");
    for stalled in ["cargo", "rustc"] {
        let directory = parent.0.join(stalled);
        let log = parent.0.join(format!("{stalled}-calls"));
        let pidfile = parent.0.join(format!("{stalled}-pid"));
        for tool in ["cargo", "rustc"] {
            fake_rust_tool(
                &parent.0,
                tool,
                if tool == stalled {
                    "printf '%s\\n' \"$$\" > \"$BLKIT_TEST_PIDFILE\"\nexec /bin/sleep 12"
                } else {
                    // When Cargo stalls, check cleanup while init is still alive, including zombies.
                    "if [ -f \"$BLKIT_TEST_PIDFILE\" ]; then\n\
                     read -r pid < \"$BLKIT_TEST_PIDFILE\"\n\
                     /bin/kill -0 \"$pid\" 2>/dev/null && exit 93\nfi\nexit 0"
                },
            );
        }
        let started = Instant::now();
        let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .arg("--no-vcs")
            .env("PATH", parent.0.join("bin"))
            .env("RUSTUP_AUTO_INSTALL", "1")
            .env("BLKIT_TEST_TOOL_LOG", &log)
            .env("BLKIT_TEST_PIDFILE", &pidfile)
            .output()
            .unwrap();
        let elapsed = started.elapsed();
        assert_tool_warnings(&output, &[stalled]);
        assert!(String::from_utf8_lossy(&output.stderr).contains("timed out"));
        assert!(elapsed >= Duration::from_secs(5), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        let pid = fs::read_to_string(pidfile).unwrap();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .output()
                .unwrap()
                .status
                .success(),
            "{stalled} child still exists: {pid}"
        );
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            format!(
                "cargo|1|--version|0|{0}\nrustc|1|--version|0|{0}\n",
                directory.display()
            )
        );
        assert_scaffold(&directory, stalled);
    }
}

fn assert_required_git_failure(output: &Output, directory: &Path, operation: &str) {
    assert_init_failure(output, operation);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(directory.to_str().unwrap()), "{stderr}");
    assert!(stderr.contains("--no-vcs"), "{stderr}");
}

#[test]
fn init_missing_git_fails_before_scaffolding_and_allows_no_vcs_recovery() {
    let parent = InitDirectory::new("missing-git");
    let directory = parent.0.join("my-app");
    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(&directory)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_required_git_failure(&output, &directory, "check Git availability");
    assert!(String::from_utf8_lossy(&output.stderr).contains("install Git"));
    assert_no_new_output(&directory, "");

    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(&directory)
        .arg("--no-vcs")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_scaffold(&directory, "my-app");
}

#[cfg(unix)]
#[test]
fn init_nonexecutable_git_fails_before_scaffolding() {
    use std::os::unix::fs::PermissionsExt;

    let parent = InitDirectory::new("nonexecutable-git");
    let mut command = fake_git(&parent.0, "exit 0");
    fs::set_permissions(parent.0.join("bin/git"), fs::Permissions::from_mode(0o644)).unwrap();
    let directory = parent.0.join("my-app");
    let output = command.arg("init").arg(&directory).output().unwrap();
    assert_required_git_failure(&output, &directory, "check Git availability");
    assert_no_new_output(&directory, "");
    assert!(!parent.0.join("git-calls").exists());
}

#[cfg(unix)]
#[test]
fn init_failing_git_probe_fails_before_scaffolding() {
    let parent = InitDirectory::new("failing-git-probe");
    let directory = parent.0.join("my-app");
    let output = fake_git(&parent.0, "echo 'fixture probe failure' >&2; exit 73")
        .arg("init")
        .arg(&directory)
        .output()
        .unwrap();
    assert_required_git_failure(&output, &directory, "check Git availability");
    assert!(String::from_utf8_lossy(&output.stderr).contains("fixture probe failure"));
    assert_no_new_output(&directory, "");
    assert_eq!(
        fs::read_to_string(parent.0.join("git-calls")).unwrap(),
        "--version\n"
    );
}

#[cfg(unix)]
#[test]
fn init_failing_git_init_reports_required_operation_without_rollback_or_staging() {
    let parent = InitDirectory::new("failing-git-init");
    let directory = parent.0.join("my-app");
    let output = fake_git(
        &parent.0,
        "if [ \"$1\" = --version ]; then exit 0; fi\necho 'fixture init failure' >&2\nexit 74",
    )
    .arg("init")
    .arg(&directory)
    .output()
    .unwrap();
    assert_required_git_failure(&output, &directory, "initialize Git repository");
    assert!(String::from_utf8_lossy(&output.stderr).contains("fixture init failure"));
    assert_scaffold(&directory, "my-app");
    let calls = fs::read_to_string(parent.0.join("git-calls")).unwrap();
    let calls: Vec<_> = calls.lines().collect();
    assert_eq!(calls.len(), 2, "unexpected Git operations: {calls:?}");
    assert_eq!(calls[0], "--version");
    assert!(calls[1].split_whitespace().any(|arg| arg == "init"));
    assert!(
        !calls[1]
            .split_whitespace()
            .any(|arg| matches!(arg, "add" | "commit"))
    );
}

#[cfg(unix)]
#[test]
fn init_git_disappearing_after_probe_reports_late_spawn_failure() {
    let parent = InitDirectory::new("disappearing-git");
    let directory = parent.0.join("my-app");
    let output = fake_git(&parent.0, "/bin/rm -- \"$0\"; exit 0")
        .arg("init")
        .arg(&directory)
        .output()
        .unwrap();
    assert_required_git_failure(&output, &directory, "initialize Git repository");
    assert_scaffold(&directory, "my-app");
    assert_eq!(
        fs::read_to_string(parent.0.join("git-calls")).unwrap(),
        "--version\n"
    );
}

#[cfg(unix)]
#[test]
fn init_known_scaffold_conflicts_precede_all_git_calls() {
    let parent = InitDirectory::new("conflicts-before-git");
    let mut command = fake_git(&parent.0, "exit 99");
    for tool in ["cargo", "rustc"] {
        fake_rust_tool(&parent.0, tool, "exit 0");
    }
    command.env("BLKIT_TEST_TOOL_LOG", parent.0.join("tool-calls"));
    for filename in ["blkit.toml", "main.bl"] {
        let directory = parent.0.join(filename.replace('.', "-"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join(filename), b"user data").unwrap();
        let output = command.arg("init").arg(&directory).output().unwrap();
        assert_init_failure(&output, filename);
        assert_eq!(fs::read(directory.join(filename)).unwrap(), b"user data");
        assert_no_new_output(&directory, filename);
        // A fresh command avoids accumulating arguments between invocations.
        command = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"));
        command
            .env("PATH", parent.0.join("bin"))
            .env("BLKIT_TEST_GIT_LOG", parent.0.join("git-calls"))
            .env("BLKIT_TEST_TOOL_LOG", parent.0.join("tool-calls"));
    }
    assert!(!parent.0.join("git-calls").exists());
    assert!(!parent.0.join("tool-calls").exists());
}

#[cfg(unix)]
#[test]
fn init_no_vcs_and_existing_worktree_markers_skip_all_git_calls() {
    let parent = InitDirectory::new("skip-git-calls");
    let mut command = fake_git(&parent.0, "exit 99");
    for (name, marker) in [
        ("opt-out", None),
        ("ordinary", Some(false)),
        ("linked", Some(true)),
    ] {
        let directory = parent.0.join(name);
        fs::create_dir(&directory).unwrap();
        match marker {
            Some(true) => fs::write(directory.join(".git"), b"gitdir: ../metadata\n").unwrap(),
            Some(false) => fs::create_dir(directory.join(".git")).unwrap(),
            None => {}
        }
        command.arg("init").arg(directory.join("my-app"));
        if marker.is_none() {
            command.arg("--no-vcs");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_scaffold(&directory.join("my-app"), "my-app");
        command = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"));
        command
            .env("PATH", parent.0.join("bin"))
            .env("BLKIT_TEST_GIT_LOG", parent.0.join("git-calls"));
    }
    assert!(!parent.0.join("git-calls").exists());
}

#[cfg(unix)]
#[test]
fn init_pins_repository_destination_despite_git_redirection_environment_and_config() {
    let parent = InitDirectory::new("git-redirection");
    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let global_config = parent.0.join("global-config");
    fs::write(&global_config, "[init]\n defaultBranch = fixture-main\n").unwrap();
    let mut command = fake_git(&parent.0, "exec \"$BLKIT_TEST_REAL_GIT\" \"$@\"");
    for setting in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_INDEX_FILE",
        "GIT_TEMPLATE_DIR",
        "core.bare",
        "core.worktree",
        "init.templateDir",
        "GIT_CONFIG_PARAMETERS",
    ] {
        let directory = parent
            .0
            .join(setting.to_ascii_lowercase().replace('.', "-"));
        let outside = parent.0.join(format!("outside-{setting}"));
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"unrelated user data").unwrap();
        // Templates can persist core.worktree even when init has an explicit work tree.
        if matches!(setting, "GIT_TEMPLATE_DIR" | "init.templateDir") {
            fs::write(
                outside.join("config"),
                format!("[core]\n bare = true\n worktree = {}\n", outside.display()),
            )
            .unwrap();
        }
        let before = metadata_snapshot(&outside);
        command
            .env("BLKIT_TEST_REAL_GIT", &real_git)
            .env("GIT_CONFIG_GLOBAL", &global_config)
            .env("GIT_CONFIG_COUNT", "0")
            .arg("init")
            .arg(&directory);
        if setting == "GIT_CONFIG_PARAMETERS" {
            command.env(
                setting,
                format!("'core.bare=true' 'core.worktree={}'", outside.display()),
            );
        } else if setting.contains('.') {
            command
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", setting)
                .env(
                    "GIT_CONFIG_VALUE_0",
                    if setting == "core.bare" {
                        std::ffi::OsStr::new("true")
                    } else {
                        outside.as_os_str()
                    },
                );
        } else {
            command.env(setting, &outside);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{setting}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            directory.join(".git").is_dir(),
            "{setting}: missing destination repository"
        );
        assert_eq!(
            metadata_snapshot(&outside),
            before,
            "{setting}: modified unrelated location"
        );
        assert_eq!(
            fixture_git(&directory, &["rev-parse", "--show-toplevel"]).stdout,
            format!("{}\n", directory.display()).as_bytes()
        );
        assert_eq!(
            fixture_git(&directory, &["symbolic-ref", "HEAD"]).stdout,
            b"refs/heads/fixture-main\n"
        );
        assert_eq!(
            fixture_git(
                &directory,
                &["status", "--porcelain", "--untracked-files=all"]
            )
            .stdout,
            b"?? blkit.toml\n?? main.bl\n"
        );
        assert!(fixture_git(&directory, &["for-each-ref"]).stdout.is_empty());
        command = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"));
        command
            .env("PATH", parent.0.join("bin"))
            .env("BLKIT_TEST_GIT_LOG", parent.0.join("git-calls"));
    }
    let calls = fs::read_to_string(parent.0.join("git-calls")).unwrap();
    assert_eq!(
        calls.lines().count(),
        22,
        "unexpected Git operations: {calls}"
    );
    assert!(
        !calls
            .split_whitespace()
            .any(|arg| matches!(arg, "add" | "commit"))
    );
}

#[test]
fn init_defaults_to_git_with_unstaged_uncommitted_scaffold() {
    let parent = InitDirectory::new("default-git");
    // Spaces and shell metacharacters in an ancestor must remain literal path bytes.
    let base = parent.0.join("literal parent ; space");
    fs::create_dir(&base).unwrap();
    let directory = base.join("my-app");
    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(&directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.join(".git").is_dir());
    assert_eq!(
        fixture_git(&directory, &["symbolic-ref", "HEAD"]).stdout,
        b"refs/heads/fixture-main\n"
    );
    assert_eq!(
        fixture_git(
            &directory,
            &["status", "--porcelain", "--untracked-files=all"]
        )
        .stdout,
        b"?? blkit.toml\n?? main.bl\n"
    );
    assert!(
        fixture_git(&directory, &["ls-files", "--stage"])
            .stdout
            .is_empty()
    );
    assert!(fixture_git(&directory, &["for-each-ref"]).stdout.is_empty());
    assert!(!directory.join(".gitignore").exists());
    Project::load(&directory).unwrap();
}

#[test]
fn init_no_vcs_succeeds_without_git_and_creates_no_metadata() {
    let parent = InitDirectory::new("no-git-opt-out");
    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "my-app", "--no-vcs"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_tool_warnings(&output, &["cargo", "rustc"]);
    assert_scaffold(&parent.0.join("my-app"), "my-app");
}

#[test]
fn init_in_existing_repository_or_parent_needs_no_git_and_preserves_metadata() {
    let parent = InitDirectory::new("parent-git");
    fixture_git(&parent.0, &["init"]);
    fs::write(parent.0.join(".gitignore"), b"private\n!keep\n").unwrap();
    fs::write(parent.0.join("notes.txt"), b"user data\0\xff").unwrap();
    fixture_git(&parent.0, &["config", "blkit.fixture", "preserve me"]);
    let before = metadata_snapshot(&parent.0.join(".git"));
    for destination in [parent.0.clone(), parent.0.join("nested/my-app")] {
        let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&destination)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if destination != parent.0 {
            assert!(!destination.join(".git").exists());
        }
    }
    assert_eq!(metadata_snapshot(&parent.0.join(".git")), before);
    assert_eq!(
        fs::read(parent.0.join(".gitignore")).unwrap(),
        b"private\n!keep\n"
    );
    assert_eq!(
        fs::read(parent.0.join("notes.txt")).unwrap(),
        b"user data\0\xff"
    );
    assert!(
        fixture_git(&parent.0, &["ls-files", "--stage"])
            .stdout
            .is_empty()
    );
}

#[test]
fn init_in_linked_worktree_or_parent_needs_no_git_and_preserves_gitfile() {
    let parent = InitDirectory::new("linked-worktree");
    let repository = parent.0.join("repository");
    fs::create_dir(&repository).unwrap();
    fixture_git(&repository, &["init"]);
    fixture_git(
        &repository,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "fixture",
        ],
    );
    fixture_git(
        &repository,
        &["worktree", "add", "--detach", "../linked", "HEAD"],
    );
    let worktree = parent.0.join("linked");
    let gitfile = fs::read(worktree.join(".git")).unwrap();
    fs::write(worktree.join(".gitignore"), b"/private\n").unwrap();
    let before = metadata_snapshot(&repository.join(".git"));
    for destination in [worktree.clone(), worktree.join("nested/my-app")] {
        let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&destination)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if destination != worktree {
            assert!(!destination.join(".git").exists());
        }
    }
    assert_eq!(fs::read(worktree.join(".git")).unwrap(), gitfile);
    assert_eq!(metadata_snapshot(&repository.join(".git")), before);
    assert_eq!(
        fs::read(worktree.join(".gitignore")).unwrap(),
        b"/private\n"
    );
    assert!(
        fixture_git(&worktree, &["diff", "--cached", "--name-only"])
            .stdout
            .is_empty()
    );
    assert_eq!(
        fixture_git(&worktree, &["rev-list", "--count", "HEAD"]).stdout,
        b"1\n"
    );
}

#[test]
fn init_does_not_treat_unrelated_child_repositories_as_parents() {
    let directory = InitDirectory::new("child-git");
    let child = directory.0.join("unrelated");
    fs::create_dir(&child).unwrap();
    fixture_git(&child, &["init"]);
    let before = metadata_snapshot(&child.join(".git"));
    let output = git_fixture_command(&directory.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.0.join(".git").is_dir());
    assert_eq!(metadata_snapshot(&child.join(".git")), before);
}

#[cfg(unix)]
#[test]
fn init_resolves_symlinked_parents_before_detecting_existing_repositories() {
    use std::os::unix::fs::symlink;

    let parent = InitDirectory::new("symlink-parent-git");
    let repository = parent.0.join("repository");
    fs::create_dir_all(repository.join("subdirectory")).unwrap();
    fixture_git(&repository, &["init"]);
    let before = metadata_snapshot(&repository.join(".git"));
    let alias = parent.0.join("alias");
    symlink(repository.join("subdirectory"), &alias).unwrap();
    let output = git_fixture_command(&parent.0, env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(alias.join("nested/my-app"))
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let destination = repository.join("subdirectory/nested/my-app");
    assert_scaffold(&destination, "my-app");
    assert_eq!(metadata_snapshot(&repository.join(".git")), before);
}

#[test]
fn init_current_directory_creates_standalone_local_project() {
    let directory = InitDirectory::new("current");
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "--no-vcs"])
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "init failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    let name = directory.0.file_name().unwrap().to_str().unwrap();
    assert_scaffold(&directory.0, name);
}

#[test]
fn initialized_project_transpiles_without_rust_tools() {
    let parent = InitDirectory::new("transpile");
    let directory = parent.0.join("my-app");
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "my-app", "--no-vcs"])
        .current_dir(&parent.0)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "init failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&directory, "my-app");

    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("transpile")
        .current_dir(&directory)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "transpile failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest = fs::read_to_string(directory.join(".blkit/Cargo.toml")).unwrap();
    assert!(manifest.contains("local-persistence"), "{manifest}");
    assert!(!manifest.contains("remote-persistence"), "{manifest}");
    assert!(directory.join(".blkit/src/lib.rs").is_file());
    assert!(
        directory
            .join(".blkit/src/bin/my-app-api-worker.rs")
            .is_file()
    );
    assert!(!directory.join(".blkit/target").exists());
    assert!(!directory.join(".blkit/Cargo.lock").exists());
    assert!(!directory.join("Cargo.toml").exists());
    assert!(!directory.join("blkit.db").exists());
}

#[test]
fn init_missing_named_directory_creates_standalone_local_project() {
    let parent = InitDirectory::new("named");
    let directory = parent.0.join("my-app");
    assert!(!directory.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "my-app", "--no-vcs"])
        .current_dir(&parent.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "init failed ({}): {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&directory, "my-app");
}

#[test]
fn init_no_vcs_accepts_following_positional_destination() {
    let parent = InitDirectory::new("no-vcs-positional");
    for name in ["git", "none"] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .args(["init", "--no-vcs", name])
            .current_dir(&parent.0)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_scaffold(&parent.0.join(name), name);
    }
}

#[test]
fn init_preflights_both_scaffold_files_without_replacing_bytes() {
    let parent = InitDirectory::new("existing-files");
    for (index, filename) in ["blkit.toml", "main.bl"].into_iter().enumerate() {
        let directory = parent.0.join(format!("project-{index}"));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join(filename), b"user edits\0\xff").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, filename);
        assert_eq!(
            fs::read(directory.join(filename)).unwrap(),
            b"user edits\0\xff"
        );
        assert_no_new_output(&directory, filename);
    }
}

#[test]
fn init_preflights_directories_at_either_scaffold_path() {
    let parent = InitDirectory::new("existing-directories");
    for (index, filename) in ["blkit.toml", "main.bl"].into_iter().enumerate() {
        let directory = parent.0.join(format!("project-{index}"));
        let conflict = directory.join(filename);
        fs::create_dir_all(&conflict).unwrap();
        fs::write(conflict.join("keep"), b"user data").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, filename);
        assert_eq!(fs::read(conflict.join("keep")).unwrap(), b"user data");
        assert_no_new_output(&directory, filename);
    }
}

#[cfg(unix)]
#[test]
fn init_preflights_symlinks_including_dangling_scaffold_paths() {
    use std::os::unix::fs::symlink;

    let parent = InitDirectory::new("existing-symlinks");
    for (index, filename) in ["blkit.toml", "main.bl"].into_iter().enumerate() {
        for dangling in [false, true] {
            let directory = parent.0.join(format!("project-{index}-{dangling}"));
            fs::create_dir(&directory).unwrap();
            let target = parent.0.join(format!("target-{index}-{dangling}"));
            if !dangling {
                fs::write(&target, b"linked user data").unwrap();
            }
            let conflict = directory.join(filename);
            symlink(&target, &conflict).unwrap();
            let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
                .arg("init")
                .arg(&directory)
                .output()
                .unwrap();
            assert_init_failure(&result, filename);
            assert_eq!(fs::read_link(&conflict).unwrap(), target);
            if dangling {
                assert!(!target.exists());
            } else {
                assert_eq!(fs::read(&target).unwrap(), b"linked user data");
            }
            assert_no_new_output(&directory, filename);
        }
    }
}

#[test]
fn repeated_init_preserves_edited_scaffold() {
    let directory = InitDirectory::new("repeated");
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "--no-vcs"])
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert!(result.status.success());
    fs::write(directory.0.join("blkit.toml"), b"edited manifest").unwrap();
    fs::write(directory.0.join("main.bl"), b"edited starter").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert_init_failure(&result, "blkit.toml");
    assert_eq!(
        fs::read(directory.0.join("blkit.toml")).unwrap(),
        b"edited manifest"
    );
    assert_eq!(
        fs::read(directory.0.join("main.bl")).unwrap(),
        b"edited starter"
    );
    assert!(!directory.0.join(".git").exists());
}

#[test]
fn init_preserves_unrelated_files_ignore_rules_and_repository_metadata() {
    let directory = InitDirectory::new("unrelated");
    fs::write(directory.0.join("notes.txt"), b"user notes\0\xff").unwrap();
    fs::write(directory.0.join(".gitignore"), b"/private\n!keep\n").unwrap();
    fs::create_dir(directory.0.join(".git")).unwrap();
    fs::write(directory.0.join(".git/config"), b"existing configuration").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "--no-vcs"])
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(directory.0.join("notes.txt")).unwrap(),
        b"user notes\0\xff"
    );
    assert_eq!(
        fs::read(directory.0.join(".gitignore")).unwrap(),
        b"/private\n!keep\n"
    );
    assert_eq!(
        fs::read(directory.0.join(".git/config")).unwrap(),
        b"existing configuration"
    );
    Project::load(&directory.0).unwrap();
}

#[test]
fn init_explicit_dot_and_trailing_separators_use_effective_directory_name() {
    let parent = InitDirectory::new("path-spellings");
    let current = parent.0.join("dot-project");
    fs::create_dir(&current).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", ".", "--no-vcs"])
        .current_dir(&current)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&current, "dot-project");

    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .args(["init", "nested/my-app/", "--no-vcs"])
        .current_dir(&parent.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&parent.0.join("nested/my-app"), "my-app");
}

#[test]
fn init_rejects_unusable_package_and_crate_names_before_writing() {
    let parent = InitDirectory::new("invalid-names");
    for name in [
        "two words",
        "bad.name",
        #[cfg(unix)]
        "quoted\"name",
        "café",
        "123app",
        "-app",
        "_",
        "-",
    ] {
        for existing in [false, true] {
            let base = parent.0.join(if existing { "existing" } else { "missing" });
            fs::create_dir_all(&base).unwrap();
            let directory = base.join(name);
            if existing {
                fs::create_dir(&directory).unwrap();
                fs::write(directory.join("keep"), b"user data").unwrap();
            }
            let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
                .arg("init")
                .arg(&directory)
                .output()
                .unwrap();
            assert_init_failure(&result, "choose");
            assert_no_new_output(&directory, "");
            if existing {
                assert_eq!(fs::read(directory.join("keep")).unwrap(), b"user data");
            }
        }
    }
}

#[test]
fn init_rejects_reserved_package_and_rust_identifiers() {
    let parent = InitDirectory::new("reserved-names");
    for name in [
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do",
        "final", "gen", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
        "try", "test",
    ] {
        let directory = parent.0.join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, "choose");
        assert_no_new_output(&directory, "");
    }
}

fn assert_generated_crate_collisions_rejected(existing: bool) {
    let parent = InitDirectory::new(if existing {
        "existing-crate-collisions"
    } else {
        "missing-crate-collisions"
    });
    for name in [
        "std",
        "core",
        "blkit-core",
        "blkit_core",
        "tokio",
        "axum",
        "serde",
        "serde_json",
        "serde-json",
        "chrono",
        "rust_decimal",
        "rust-decimal",
        "env",
        "logging",
        "tracing",
        "Path",
        "Arc",
        "AttachmentStore",
        "Engine",
        "Registry",
        "LocalStore",
    ] {
        let directory = parent.0.join(name);
        if existing {
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("keep"), b"user data\0\xff").unwrap();
        }
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, "choose");
        assert!(String::from_utf8_lossy(&result.stderr).contains("generated project"));
        assert_no_new_output(&directory, "");
        if existing {
            assert_eq!(
                fs::read(directory.join("keep")).unwrap(),
                b"user data\0\xff"
            );
        } else {
            assert!(
                !directory.exists(),
                "invalid destination was created: {name}"
            );
        }
    }
}

#[test]
fn init_rejects_generated_crate_collisions_in_existing_destinations() {
    assert_generated_crate_collisions_rejected(true);
}

#[test]
fn init_rejects_generated_crate_collisions_in_missing_destinations() {
    assert_generated_crate_collisions_rejected(false);
}

#[test]
fn init_accepts_noncolliding_warning_only_and_similarly_named_packages() {
    let parent = InitDirectory::new("noncolliding-crate-names");
    for name in [
        "alloc",
        "proc_macro",
        "proc-macro",
        "build",
        "tokio-app",
        "blkit-core-app",
        "env-app",
        "path",
        "arc",
        "registry",
        "router_with_attachments",
    ] {
        let directory = parent.0.join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .args(["--no-vcs"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_scaffold(&directory, name);
    }
}

#[test]
fn init_accepts_generated_filename_length_boundary_and_transpiles() {
    let parent = InitDirectory::new("filename-length-boundary");
    let name = "a".repeat(216);
    for existing in [false, true] {
        let base = parent.0.join(if existing { "existing" } else { "missing" });
        fs::create_dir(&base).unwrap();
        let directory = base.join(&name);
        if existing {
            fs::create_dir(&directory).unwrap();
        }
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .arg("--no-vcs")
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_scaffold(&directory, &name);
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("transpile")
            .arg(&directory)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert!(
            directory
                .join(format!(".blkit/src/bin/{name}-api-worker.rs"))
                .is_file()
        );
    }
}

fn assert_generated_filename_overflow_rejected(existing: bool) {
    let parent = InitDirectory::new(if existing {
        "existing-filename-overflow"
    } else {
        "missing-filename-overflow"
    });
    for length in [217, 241, 242] {
        let directory = parent.0.join("a".repeat(length));
        if existing {
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("keep"), b"user data\0\xff").unwrap();
        }
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, "choose");
        assert!(String::from_utf8_lossy(&result.stderr).contains("generated"));
        assert_no_new_output(&directory, "");
        if existing {
            assert_eq!(
                metadata_snapshot(&directory),
                [(PathBuf::from("keep"), b"user data\0\xff".to_vec())]
            );
        } else {
            assert!(!directory.exists(), "invalid destination was created");
        }
    }
}

#[test]
fn init_rejects_generated_filename_overflow_in_existing_destinations() {
    assert_generated_filename_overflow_rejected(true);
}

#[test]
fn init_rejects_generated_filename_overflow_in_missing_destinations() {
    assert_generated_filename_overflow_rejected(false);
}

#[test]
fn init_rejects_non_directory_destinations_with_guidance() {
    let parent = InitDirectory::new("invalid-destinations");
    let file = parent.0.join("file");
    fs::write(&file, b"user bytes\0\xff").unwrap();
    for destination in [&file, &file.join("child")] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(destination)
            .output()
            .unwrap();
        assert_init_failure(&result, "choose");
        assert_eq!(fs::read(&file).unwrap(), b"user bytes\0\xff");
        assert_no_new_output(&parent.0, "");
    }
}

#[test]
fn init_rejects_destinations_without_a_directory_name() {
    let directory = InitDirectory::new("missing-name");
    let root = directory.0.ancestors().last().unwrap();
    // Clap rejects an explicitly empty argument before initializer dispatch.
    for (destination, diagnostic) in [(Path::new(""), "Usage:"), (root, "choose")] {
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(destination)
            .current_dir(&directory.0)
            .output()
            .unwrap();
        assert_init_failure(&result, diagnostic);
        assert_no_new_output(&directory.0, "");
    }
}

#[cfg(unix)]
#[test]
fn init_rejects_non_utf8_directory_names_without_writing() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let parent = InitDirectory::new("non-utf8");
    for existing in [false, true] {
        let base = parent.0.join(if existing { "existing" } else { "missing" });
        fs::create_dir(&base).unwrap();
        let directory = base.join(OsStr::from_bytes(b"invalid-\xff"));
        if existing {
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("keep"), b"user data").unwrap();
        }
        let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert_init_failure(&result, "UTF-8");
        assert_no_new_output(&directory, "");
        if existing {
            assert_eq!(fs::read(directory.join("keep")).unwrap(), b"user data");
        }
    }
}

#[cfg(unix)]
#[test]
fn init_derives_names_from_resolved_destination_and_symlinked_parents() {
    use std::os::unix::fs::symlink;

    let parent = InitDirectory::new("resolved-names");
    let directory = parent.0.join("actual-project");
    fs::create_dir(&directory).unwrap();
    let alias = parent.0.join("alias");
    symlink(&directory, &alias).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(&alias)
        .args(["--no-vcs"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&directory, "actual-project");

    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(alias.join("child-project"))
        .args(["--no-vcs"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_scaffold(&directory.join("child-project"), "child-project");

    let invalid = parent.0.join("123invalid");
    fs::create_dir(&invalid).unwrap();
    let alias = parent.0.join("valid-alias");
    symlink(&invalid, &alias).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("init")
        .arg(alias)
        .output()
        .unwrap();
    assert_init_failure(&result, "choose");
    assert_no_new_output(&invalid, "");
}
