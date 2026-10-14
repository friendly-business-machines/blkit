use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use indicatif::ProgressBar;

#[derive(Parser)]
#[command(
    version,
    about = "Transpile .bl sources and projects to Rust",
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
    Transpile { project_dir: Option<PathBuf> },
    Update { project_dir: Option<PathBuf> },
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
    source_code: Option<miette::NamedSource<String>>,
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

fn main() {
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "build") {
        Cli::command()
            .error(
                clap::error::ErrorKind::InvalidSubcommand,
                "'build' was removed; use 'transpile' and run cargo build separately",
            )
            .exit();
    }
    let cli = Cli::parse();
    if let Some(shell) = cli.completions {
        clap_complete::generate(shell, &mut Cli::command(), "blkit", &mut std::io::stdout());
        return;
    }
    let result = match cli.command {
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
