use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use blkit_cli::postgres_admin::{ManagedRoles, Preparation, PreparationOutcome, prepare_database};
use blkit_core::postgres_config::EffectivePostgresConfig;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use indicatif::ProgressBar;

#[derive(Parser)]
#[command(
    version,
    about = "Transpile .bl sources and projects to Rust; administer PostgreSQL deployments",
    after_help = format!("Database administration:\n  blkit db init     Initialize schema and managed roles\n  blkit db migrate  Upgrade schema and reconcile grants\n\n{DB_HELP}"),
    args_conflicts_with_subcommands = true,
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Operation>,
    #[arg(
        value_name = "SOURCE.bl",
        requires = "output",
        conflicts_with = "completions"
    )]
    source: Option<PathBuf>,
    #[arg(
        value_name = "OUTPUT.rs",
        requires = "source",
        conflicts_with = "completions"
    )]
    output: Option<PathBuf>,
    #[arg(long, value_name = "SHELL", conflicts_with_all = ["source", "output"])]
    completions: Option<Shell>,
}

#[derive(Subcommand)]
enum Operation {
    Transpile {
        project_dir: Option<PathBuf>,
    },
    Update {
        project_dir: Option<PathBuf>,
    },
    /// Initialize or migrate an existing dedicated PostgreSQL database
    #[command(long_about = DB_HELP)]
    Db {
        #[command(subcommand)]
        command: DatabaseOperation,
    },
}

const DB_HELP: &str =
    "Prepare an existing, dedicated PostgreSQL database; does not create a server or database.
No project, Cargo, .bl sources, or cloud credentials are required.

Administration uses BLKIT_ADMIN_DATABASE_URL or BLKIT_ADMIN_DATABASE_URL_FILE only.
New runtime logins require BLKIT_API_DATABASE_URL and BLKIT_WORKER_DATABASE_URL (or
corresponding _FILE settings), with the same database endpoint and managed usernames.
Existing login passwords are never reset; prepared reruns normally need only admin credentials.
Never supply database URLs, usernames or passwords as arguments. URL-encode special characters.

Use process environment or an explicitly selected --env-file PATH (dotenv data, no
shell execution or automatic discovery). Process settings override dotenv by name.
A required URL and its _FILE counterpart conflict; secret files contain one UTF-8 URL
with an optional trailing LF/CRLF. Paths are relative to the working directory.

TLS verifies certificates and hostnames using platform trust plus the optional
BLKIT_POSTGRES_CA_FILE corporate PEM CA bundle. BLKIT_POSTGRES_INSECURE_DEV=true
explicitly selects plaintext for development/testing only and conflicts with a CA file.

--role-prefix selects <prefix>_owner (NOLOGIN), <prefix>_api and <prefix>_worker.
Prefixes use 1-56 lowercase ASCII letters, digits or underscores, start with a letter
or underscore, and cannot start with pg_. Init defaults to blkit; migrate without a
prefix uses the persisted identity (blkit for first legacy adoption). Existing role
name conflicts are refused. Migration requires quiesced legacy writers; only recognized
blkit schemas are adopted, with application data preserved.";

#[derive(Subcommand)]
enum DatabaseOperation {
    /// Install schema and roles in an empty database, or reconcile a prepared database
    #[command(long_about = DB_HELP)]
    Init(DatabaseArguments),
    /// Upgrade a recognized legacy schema, or reconcile a prepared database
    #[command(long_about = DB_HELP)]
    Migrate(DatabaseArguments),
}

#[derive(Args)]
struct DatabaseArguments {
    /// Explicit dotenv file; process environment takes precedence
    #[arg(long, value_name = "PATH")]
    env_file: Option<PathBuf>,
    /// Managed role prefix; migrate omission preserves the recorded identity
    #[arg(long, value_name = "PREFIX", value_parser = parse_role_prefix)]
    role_prefix: Option<String>,
}

fn parse_role_prefix(value: &str) -> Result<String, String> {
    ManagedRoles::from_prefix(value)
        .map(|_| value.to_owned())
        .map_err(|error| error.to_string())
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("{operation} {}: {details}", path.display())]
struct CliError {
    operation: &'static str,
    path: PathBuf,
    details: String,
    #[related]
    related: Vec<CompilerDiagnostic>,
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("{message}")]
struct CompilerDiagnostic {
    message: String,
    #[source_code]
    source_code: Option<miette::NamedSource<std::sync::Arc<str>>>,
    #[label("here")]
    span: Option<miette::SourceSpan>,
    #[related]
    related: Vec<Self>,
}

impl From<blkit_transpiler::CompilerError> for CompilerDiagnostic {
    fn from(error: blkit_transpiler::CompilerError) -> Self {
        let span = error.span.filter(|span| {
            error.source.as_ref().is_some_and(|source| {
                span.start <= span.end
                    && span.end <= source.len()
                    && source.is_char_boundary(span.start)
                    && source.is_char_boundary(span.end)
            })
        });
        let message = if error.source_name.is_empty() {
            error.message.clone()
        } else {
            format!("{}: {}", error.source_name, error.message)
        };
        Self {
            message,
            source_code: span.as_ref().and_then(|_| {
                error
                    .source
                    .map(|source| miette::NamedSource::new(error.source_name, source))
            }),
            span: span.map(|span| (span.start, span.len()).into()),
            related: error.related.into_iter().map(Self::from).collect(),
        }
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::CompilerDiagnostic;

    #[test]
    fn related_labels_reuse_compiler_source_and_render_both_utf8_locations() {
        let source = "namespace demo; version \"1\";\n# café\nstart_event start { output first: ☃; output second: ; }";
        let error = blkit_transpiler::transpile_diagnostic(source, "shared.bl").unwrap_err();
        let pointer = error.source.as_ref().unwrap().as_ptr();
        let diagnostic = CompilerDiagnostic::from(error);
        assert_eq!(
            diagnostic.source_code.as_ref().unwrap().inner().as_ptr(),
            pointer
        );
        assert_eq!(diagnostic.related.len(), 1);
        assert_eq!(
            diagnostic.related[0]
                .source_code
                .as_ref()
                .unwrap()
                .inner()
                .as_ptr(),
            pointer
        );
        let mut rendered = String::new();
        miette::NarratableReportHandler::new()
            .render_report(&mut rendered, &diagnostic)
            .unwrap();
        assert!(
            rendered.contains("snippet line 3: start_event start"),
            "{rendered}"
        );
        assert!(rendered.contains("invalid character"), "{rendered}");
        assert!(rendered.contains("missing semicolon"), "{rendered}");
        assert!(rendered.contains("shared.bl"), "{rendered}");
    }
}

fn main() {
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "build") {
        Cli::command()
            .error(
                clap::error::ErrorKind::InvalidSubcommand,
                "'build' was removed; use 'transpile' and run cargo build separately",
            )
            .exit();
    }
    // Clap's ordinary errors can echo credential-bearing operands, including unknown flags.
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.iter().skip(1).any(|argument| {
        let value = argument.to_string_lossy();
        value.contains("postgres://") || value.contains("postgresql://")
    }) {
        invalid_usage();
    }
    let cli = Cli::try_parse_from(arguments).unwrap_or_else(|error| match error.kind() {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
            error.exit()
        }
        _ => invalid_usage(),
    });
    if let Some(shell) = cli.completions {
        clap_complete::generate(shell, &mut Cli::command(), "blkit", &mut std::io::stdout());
        return;
    }
    let result = match cli.command {
        Some(Operation::Db { command }) => {
            administer_database(command);
            return;
        }
        Some(Operation::Transpile { project_dir }) => project("transpile", project_dir.as_deref()),
        Some(Operation::Update { project_dir }) => project("update", project_dir.as_deref()),
        None => {
            let source = cli.source.expect("clap requires a source or subcommand");
            let output = cli.output.expect("clap requires an output for a source");
            transpile(&source, &output)
        }
    };
    if let Err(error) = result {
        let mut diagnostic = String::new();
        miette::NarratableReportHandler::new()
            .render_report(&mut diagnostic, &error)
            .expect("writing a diagnostic to String cannot fail");
        eprint!("{diagnostic}");
        std::process::exit(1);
    }
}

fn invalid_usage() -> ! {
    Cli::command()
        .error(
            clap::error::ErrorKind::InvalidValue,
            "invalid arguments; use --help; database credentials belong in environment or secret files, not arguments",
        )
        .exit()
}

fn administer_database(command: DatabaseOperation) {
    let (arguments, init) = match command {
        DatabaseOperation::Init(arguments) => (arguments, true),
        DatabaseOperation::Migrate(arguments) => (arguments, false),
    };
    // Format only the closed, redacted config/runner errors, never driver sources or Debug.
    let result = EffectivePostgresConfig::from_env(arguments.env_file.as_deref())
        .map_err(|error| error.to_string())
        .and_then(|settings| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "cannot start database administration runtime".to_owned())?;
            let role_prefix = arguments.role_prefix.as_deref();
            let preparation = if init {
                Preparation::Init { role_prefix }
            } else {
                Preparation::Migrate { role_prefix }
            };
            runtime
                .block_on(prepare_database(&settings, preparation))
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(PreparationOutcome::Initialized) => println!("Database initialized."),
        Ok(PreparationOutcome::Migrated) => println!("Database migrated."),
        Ok(PreparationOutcome::AlreadyCurrent) => println!("Database already current."),
        Err(error) => {
            eprintln!("database preparation failed: {error}");
            std::process::exit(1);
        }
    }
}

fn transpile(source: &Path, output: &Path) -> Result<(), CliError> {
    let text = fs::read_to_string(source).map_err(|error| CliError {
        operation: "read",
        path: source.to_path_buf(),
        details: error.to_string(),
        related: Vec::new(),
    })?;
    let rust = blkit_transpiler::transpile_diagnostic(&text, &source.to_string_lossy()).map_err(
        |error| CliError {
            operation: "compile",
            path: source.to_path_buf(),
            details: error.to_string(),
            related: vec![error.into()],
        },
    )?;
    fs::write(output, rust).map_err(|error| CliError {
        operation: "write",
        path: output.to_path_buf(),
        details: error.to_string(),
        related: Vec::new(),
    })
}

fn project(operation: &'static str, directory: Option<&Path>) -> Result<(), CliError> {
    let directory = directory.unwrap_or_else(|| Path::new("."));
    let interactive = console::Term::stderr().is_term()
        && console::colors_enabled_stderr()
        && std::env::var_os("NO_COLOR").is_none();
    let progress = interactive.then(|| {
        let progress = ProgressBar::new_spinner();
        progress.set_message(format!(
            "{} {}",
            console::style(operation).cyan().force_styling(true),
            directory.display()
        ));
        progress.enable_steady_tick(Duration::from_millis(120));
        progress
    });
    let result =
        blkit_transpiler::project::Project::load(directory).map(|project| match operation {
            "transpile" => project.transpile_diagnostic().map_err(|error| CliError {
                operation,
                path: directory.to_path_buf(),
                details: error.to_string(),
                related: vec![error.into()],
            }),
            _ => project.update().map_err(|details| CliError {
                operation,
                path: directory.to_path_buf(),
                details,
                related: Vec::new(),
            }),
        });
    if let Some(progress) = progress {
        progress.finish_and_clear();
    }
    result.map_err(|details| CliError {
        operation,
        path: directory.to_path_buf(),
        details,
        related: Vec::new(),
    })?
}
