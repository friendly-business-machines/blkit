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
    })?;
    let rust = blkit::transpile(&text).map_err(|details| CliError {
        operation: "compile",
        path: source.to_path_buf(),
        details,
    })?;
    fs::write(output, rust).map_err(|error| CliError {
        operation: "write",
        path: output.to_path_buf(),
        details: error.to_string(),
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
    let result = blkit::project::Project::load(directory).and_then(|project| match operation {
        "transpile" => project.transpile(),
        _ => project.update(),
    });
    if let Some(progress) = progress {
        progress.finish_and_clear();
    }
    result.map_err(|details| CliError {
        operation,
        path: directory.to_path_buf(),
        details,
    })
}
