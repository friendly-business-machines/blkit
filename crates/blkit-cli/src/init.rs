use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::CliError;

// The local binary's Cargo hash and linker temporary suffix must fit a 255-byte component.
const MAX_PROJECT_NAME_BYTES: usize = 255 - "_api_worker-0000000000000000.tmp0000000".len();

const STARTER: &str = "namespace example;
version \"1\";

start_event start {
    output input: Number;
}

end_event done {
    input result: Number;
}

process echo {
    flow start -> done;
    bind start.input -> done.result;
}
";

pub(super) fn initialize(directory: Option<&Path>, no_vcs: bool) -> Result<(), CliError> {
    let directory = directory.unwrap_or_else(|| Path::new("."));
    let directory = match directory.canonicalize() {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => directory.to_path_buf(),
        Err(error) => {
            return Err(CliError {
                operation: "resolve directory",
                path: directory.to_path_buf(),
                details: format!("{error}; choose an accessible destination directory"),
                related: Vec::new(),
            });
        }
    };
    let invalid_name = || CliError {
        operation: "initialize",
        path: directory.clone(),
        details: format!(
            "choose or rename the destination to a UTF-8 directory name usable as a Cargo \
             package and Rust crate: use at most {MAX_PROJECT_NAME_BYTES} ASCII letters, digits, '-' or '_', \
             start with a letter or '_', and avoid names reserved by Rust or the generated project, \
             including '_'"
        ),
        related: Vec::new(),
    };
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid_name)?;
    if name.len() > MAX_PROJECT_NAME_BYTES
        || !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        // Rust 2024 crate imports cannot use keywords; Cargo also reserves `test`.
        || matches!(
            name,
            "_" | "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern"
                | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match"
                | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static"
                | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where"
                | "while" | "async" | "await" | "dyn" | "abstract" | "become" | "box" | "do"
                | "final" | "gen" | "macro" | "override" | "priv" | "typeof" | "unsized"
                | "virtual" | "yield" | "try" | "test"
        )
        // Match generated dependencies, local binary imports, and tokio's standard-library paths.
        || matches!(
            name.replace('-', "_").as_str(),
            "std" | "core" | "blkit_core" | "tokio" | "axum" | "serde" | "serde_json"
                | "chrono" | "rust_decimal" | "env" | "logging" | "tracing" | "Path" | "Arc"
                | "AttachmentStore" | "Engine" | "Registry" | "LocalStore"
        )
    {
        return Err(invalid_name());
    }
    fs::create_dir_all(&directory).map_err(|error| CliError {
        operation: "create directory",
        path: directory.clone(),
        details: format!("{error}; choose a writable destination directory"),
        related: Vec::new(),
    })?;
    let directory = directory.canonicalize().map_err(|error| CliError {
        operation: "resolve directory",
        path: directory.clone(),
        details: error.to_string(),
        related: Vec::new(),
    })?;
    for filename in ["blkit.toml", "main.bl"] {
        let path = directory.join(filename);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                return Err(CliError {
                    operation: "initialize",
                    path,
                    details: "scaffold path already exists; move it or choose another destination"
                        .into(),
                    related: Vec::new(),
                });
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError {
                    operation: "inspect scaffold path",
                    path,
                    details: error.to_string(),
                    related: Vec::new(),
                });
            }
        }
    }
    let initialize_git = !no_vcs && !in_git_worktree(&directory)?;
    if initialize_git {
        run_git(
            Command::new("git").arg("--version"),
            &directory,
            "check Git availability",
        )?;
    }
    let manifest = format!(
        "[project]\nname = \"{name}\"\nblkit = \"{}\"\nbuild_target = \"api-worker\"\npersistence = \"local\"\n",
        env!("CARGO_PKG_VERSION")
    );
    for (filename, contents) in [("blkit.toml", manifest.as_str()), ("main.bl", STARTER)] {
        let path = directory.join(filename);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| CliError {
                operation: "create scaffold file",
                path: path.clone(),
                details: error.to_string(),
                related: Vec::new(),
            })?;
        file.write_all(contents.as_bytes())
            .map_err(|error| CliError {
                operation: "write scaffold file",
                path,
                details: error.to_string(),
                related: Vec::new(),
            })?;
    }
    if initialize_git {
        run_git(
            Command::new("git")
                .arg("--git-dir")
                .arg(directory.join(".git"))
                .arg("--work-tree")
                .arg(&directory)
                // Template config can persist a redirected core.worktree despite --work-tree.
                .args(["init", "--template="]),
            &directory,
            "initialize Git repository",
        )?;
    }
    for tool in ["cargo", "rustc"] {
        let warn = |details: &dyn std::fmt::Display| {
            eprintln!(
                "warning: {tool} {details}; needed for the later Cargo build, not to author or transpile this starter"
            );
        };
        let started = Instant::now();
        let mut child = match Command::new(tool)
            .arg("--version")
            .current_dir(&directory)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                warn(&format_args!("version probe could not start: {error}"));
                continue;
            }
        };
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        warn(&format_args!("version probe failed ({status})"));
                    }
                    break;
                }
                Ok(None) if started.elapsed() < Duration::from_secs(5) => {
                    thread::sleep(Duration::from_millis(20));
                }
                result => {
                    match result {
                        Err(error) => warn(&format_args!(
                            "version probe could not be waited on: {error}"
                        )),
                        _ => warn(&"version probe timed out after five seconds"),
                    }
                    if let Err(error) = child.kill() {
                        warn(&format_args!("could not terminate version probe: {error}"));
                    }
                    if let Err(error) = child.wait() {
                        warn(&format_args!("could not reap version probe: {error}"));
                    }
                    break;
                }
            }
        }
    }
    println!("Initialized project at {directory:?}.");
    println!("Scaffold created only; the application has not been transpiled, built, or started.");
    println!("Tool availability checks do not verify compiler compatibility or build readiness.");
    println!("\nNext steps (POSIX sh):");
    if std::env::current_dir().ok().as_deref() != Some(directory.as_path()) {
        match directory
            .to_str()
            .filter(|path| !path.chars().any(char::is_control))
        {
            Some(path) => println!("  cd '{}'", path.replace('\'', "'\\''")),
            // Avoid lossy commands and raw terminal controls without rejecting valid destinations.
            None => {
                println!("Enter the destination directory manually before running these commands.")
            }
        }
    }
    println!("  blkit transpile");
    println!("  cargo build --manifest-path .blkit/Cargo.toml");
    println!("  .blkit/target/debug/{name}-api-worker");
    println!(
        "\nThe run path assumes Cargo's default target directory. If CARGO_TARGET_DIR is set, \
         run {name}-api-worker from its debug directory instead (resolve relative paths from the project directory)."
    );
    println!(
        "The local API/worker defaults to http://127.0.0.1:3000 (loopback), with no native authentication or TLS. \
         Do not expose it directly; remote access needs authenticated TLS ingress with per-instance authorization."
    );
    Ok(())
}

fn run_git(
    command: &mut Command,
    directory: &Path,
    operation: &'static str,
) -> Result<(), CliError> {
    // Pin repository storage as well as the work tree without discarding user branch config.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(variable);
    }
    let failure = |details| CliError {
        operation,
        path: directory.to_path_buf(),
        details: format!(
            "{details}; install Git or fix Git setup, or use --no-vcs to skip repository initialization"
        ),
        related: Vec::new(),
    };
    let output = command
        .current_dir(directory)
        .output()
        .map_err(|error| failure(error.to_string()))?;
    if !output.status.success() {
        return Err(failure(format!(
            "Git command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn in_git_worktree(directory: &Path) -> Result<bool, CliError> {
    // The caller canonicalizes the destination, including any symlinked parents.
    for ancestor in directory.ancestors() {
        let path = ancestor.join(".git");
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError {
                    operation: "inspect Git metadata",
                    path,
                    details: error.to_string(),
                    related: Vec::new(),
                });
            }
        }
    }
    Ok(false)
}
