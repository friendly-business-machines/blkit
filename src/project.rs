use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item};

use crate::{Program, codegen, graph::NodeKind, identifier, parse, type_ref, validate};

pub use crate::compiler::ExternalTask;

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub build_target: String,
    pub sources: Vec<PathBuf>,
    pub dependencies: BTreeMap<String, Dependency>,
}

#[derive(Debug)]
pub struct Dependency {
    pub version: String,
    pub path: Option<PathBuf>,
}

impl Project {
    pub fn load(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let text =
            fs::read_to_string(root.join("blkit.toml")).map_err(|e| format!("blkit.toml: {e}"))?;
        let manifest = text
            .parse::<DocumentMut>()
            .map_err(|e| format!("blkit.toml: {e}"))?;
        for (key, _) in manifest.iter() {
            if !matches!(key, "project" | "dependencies") {
                return Err(format!("unknown blkit.toml section: {key}"));
            }
        }
        let project = manifest
            .get("project")
            .and_then(Item::as_table)
            .ok_or("missing [project] section")?;
        for (key, _) in project.iter() {
            if !matches!(key, "name" | "blkit" | "build_target") {
                return Err(format!("unexpected project field: {key}"));
            }
        }
        let field = |name: &str| {
            project
                .get(name)
                .and_then(Item::as_str)
                .ok_or_else(|| format!("missing or invalid project.{name}"))
        };
        let name = field("name")?.to_owned();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("invalid project.name".into());
        }
        if field("blkit")? != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "blkit version mismatch: expected {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        let build_target = field("build_target")?;
        if !matches!(build_target, "crate" | "worker" | "server") {
            return Err(format!("invalid build_target: {build_target}"));
        }
        let mut dependencies = BTreeMap::new();
        if let Some(deps) = manifest.get("dependencies") {
            let deps = deps.as_table().ok_or("invalid [dependencies]")?;
            for (name, item) in deps.iter() {
                let (version, path) = if let Some(version) = item.as_str() {
                    (version, None)
                } else if let Some(table) = item.as_inline_table() {
                    if table
                        .iter()
                        .any(|(key, _)| !matches!(key, "version" | "path"))
                    {
                        return Err(format!("invalid dependency {name} fields"));
                    }
                    (
                        table
                            .get("version")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| format!("invalid dependency {name} version"))?,
                        table
                            .get("path")
                            .map(|v| {
                                v.as_str()
                                    .map(PathBuf::from)
                                    .ok_or_else(|| format!("invalid dependency {name} path"))
                            })
                            .transpose()?,
                    )
                } else {
                    return Err(format!("invalid dependency {name} version"));
                };
                if !identifier(name)
                    || version.is_empty()
                    || path.as_ref().is_some_and(|p| p.as_os_str().is_empty())
                {
                    return Err(format!("invalid dependency {name} version or path"));
                }
                dependencies.insert(
                    name.into(),
                    Dependency {
                        version: version.into(),
                        path,
                    },
                );
            }
        }
        let mut sources = Vec::new();
        discover(&root, &mut sources).map_err(|e| e.to_string())?;
        sources.sort();
        if sources.is_empty() {
            return Err("no .bl files found in project".into());
        }
        Ok(Self {
            root,
            name,
            build_target: build_target.into(),
            sources,
            dependencies,
        })
    }

    pub fn extensions(&self) -> Result<BTreeMap<String, ExternalTask>, String> {
        if self.dependencies.is_empty() {
            return Ok(BTreeMap::new());
        }
        let package = self.root.join(".blkit");
        let mut command = std::process::Command::new("cargo");
        command
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(package.join("Cargo.toml"));
        if package.join("Cargo.lock").exists() {
            command.arg("--locked");
        }
        let metadata = command
            .output()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        if !metadata.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&metadata.stderr)
            ));
        }
        let data: serde_json::Value =
            serde_json::from_slice(&metadata.stdout).map_err(|e| e.to_string())?;
        let root = data
            .pointer("/resolve/root")
            .and_then(|v| v.as_str())
            .ok_or("missing Cargo root package")?;
        let nodes = data
            .pointer("/resolve/nodes")
            .and_then(|v| v.as_array())
            .ok_or("missing Cargo dependency graph")?;
        let deps = nodes
            .iter()
            .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(root))
            .and_then(|n| n.get("deps"))
            .and_then(|v| v.as_array())
            .ok_or("missing direct Cargo dependencies")?;
        let packages = data
            .get("packages")
            .and_then(|v| v.as_array())
            .ok_or("missing Cargo packages")?;
        let mut tasks = BTreeMap::new();
        for name in self.dependencies.keys() {
            let id = deps
                .iter()
                .find(|d| d.get("name").and_then(|v| v.as_str()) == Some(name))
                .and_then(|d| d.get("pkg"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("unresolved dependency {name}"))?;
            let manifest = packages
                .iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
                .and_then(|p| p.get("manifest_path"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("missing Cargo package for {name}"))?;
            let descriptor = Path::new(manifest)
                .parent()
                .ok_or("invalid Cargo manifest path")?
                .join("blkit-tasks.toml");
            let text = fs::read_to_string(&descriptor)
                .map_err(|e| format!("{name}: {}: {e}", descriptor.display()))?;
            let document = text
                .parse::<DocumentMut>()
                .map_err(|e| format!("{name}: blkit-tasks.toml: {e}"))?;
            if document.iter().any(|(key, _)| key != "tasks") {
                return Err(format!("{name}: unknown blkit-tasks.toml field"));
            }
            let entries = document
                .get("tasks")
                .and_then(Item::as_array_of_tables)
                .ok_or_else(|| format!("{name}: missing [[tasks]] in blkit-tasks.toml"))?;
            for entry in entries {
                if entry
                    .iter()
                    .any(|(key, _)| !matches!(key, "name" | "function" | "input" | "output"))
                {
                    return Err(format!("{name}: invalid task descriptor field"));
                }
                let field = |key: &str| {
                    entry
                        .get(key)
                        .and_then(Item::as_str)
                        .ok_or_else(|| format!("{name}: invalid task {key} in blkit-tasks.toml"))
                };
                let task_name = field("name")?;
                let function = field("function")?;
                if !identifier(task_name) || !function.split("::").all(identifier) {
                    return Err(format!(
                        "{name}.{task_name}: invalid task function or name {function}"
                    ));
                }
                let input =
                    type_ref(field("input")?).map_err(|e| format!("{name}.{task_name}: {e}"))?;
                let output =
                    type_ref(field("output")?).map_err(|e| format!("{name}.{task_name}: {e}"))?;
                let qualified = format!("{name}.{task_name}");
                if tasks
                    .insert(
                        qualified.clone(),
                        ExternalTask {
                            function: function.into(),
                            input,
                            output,
                        },
                    )
                    .is_some()
                {
                    return Err(format!("duplicate task {qualified}"));
                }
            }
        }
        Ok(tasks)
    }

    pub fn programs(&self) -> Result<Vec<Program>, String> {
        type ProjectGroup = (Program, BTreeMap<String, PathBuf>, Vec<PathBuf>);
        let mut groups: BTreeMap<(String, String), ProjectGroup> = BTreeMap::new();
        for path in &self.sources {
            let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let parsed = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let key = (parsed.namespace.clone(), parsed.version.clone());
            let names = parsed
                .records
                .iter()
                .map(|d| &d.name)
                .chain(parsed.enums.iter().map(|d| &d.name))
                .chain(parsed.tasks.iter().map(|d| &d.name))
                .chain(parsed.decisions.iter().map(|d| &d.name))
                .chain(parsed.processes.iter().map(|d| &d.name));
            match groups.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let origins = names.map(|name| (name.clone(), path.clone())).collect();
                    entry.insert((parsed, origins, vec![path.clone()]));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let (merged, origins, paths) = entry.get_mut();
                    for name in names {
                        if let Some(previous) = origins.insert(name.clone(), path.clone()) {
                            return Err(format!(
                                "duplicate declaration {name}: {} and {}",
                                previous.display(),
                                path.display()
                            ));
                        }
                    }
                    paths.push(path.clone());
                    merged.records.extend(parsed.records);
                    merged.enums.extend(parsed.enums);
                    merged.tasks.extend(parsed.tasks);
                    merged.decisions.extend(parsed.decisions);
                    merged.processes.extend(parsed.processes);
                }
            }
        }
        let needs_extensions = groups.values().any(|(program, _, _)| {
            program
                .processes
                .iter()
                .filter_map(|process| process.named_graph.as_ref())
                .flat_map(|graph| &graph.nodes)
                .any(|node| match &node.kind {
                    NodeKind::Task { task, .. }
                    | NodeKind::MultiInstance { task, .. }
                    | NodeKind::TaskLoop { task, .. } => task.contains('.'),
                    _ => false,
                })
        });
        let extensions = if needs_extensions {
            self.prepare_manifest()?;
            self.extensions()?
        } else {
            BTreeMap::new()
        };
        let mut programs = Vec::new();
        for (_, (mut program, _, paths)) in groups {
            for task in program
                .processes
                .iter()
                .filter_map(|p| p.named_graph.as_ref())
                .flat_map(|g| &g.nodes)
                .filter_map(|n| match &n.kind {
                    NodeKind::Task { task, .. }
                    | NodeKind::MultiInstance { task, .. }
                    | NodeKind::TaskLoop { task, .. }
                        if task.contains('.') =>
                    {
                        Some(task)
                    }
                    _ => None,
                })
            {
                if let Some(definition) = extensions.get(task) {
                    program
                        .external_tasks
                        .insert(task.clone(), definition.clone());
                }
            }
            validate(&program).map_err(|e| {
                format!(
                    "{}: {e}",
                    paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            programs.push(program);
        }
        Ok(programs)
    }

    pub fn build(&self) -> Result<(), String> {
        let package = self.generate()?;
        let mut command = std::process::Command::new("cargo");
        command
            .args(["build", "--manifest-path"])
            .arg(package.join("Cargo.toml"));
        if package.join("Cargo.lock").exists() {
            command.arg("--locked");
        }
        let result = command
            .output()
            .map_err(|e| format!("running cargo: {e}"))?;
        if !result.status.success() {
            return Err(format!(
                "cargo build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        Ok(())
    }

    pub fn update(&self) -> Result<(), String> {
        // Explicit updates must resolve new requirements before reading locked extension metadata.
        let package = self.prepare_manifest()?;
        let result = std::process::Command::new("cargo")
            .args(["update", "--manifest-path"])
            .arg(package.join("Cargo.toml"))
            .output()
            .map_err(|e| format!("running cargo update: {e}"))?;
        if !result.status.success() {
            return Err(format!(
                "cargo update failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        Ok(())
    }

    fn generate(&self) -> Result<PathBuf, String> {
        let programs = self.programs()?;
        let generated: Vec<_> = programs
            .iter()
            .map(codegen::generate)
            .collect::<Result<_, _>>()?;
        let package = self.prepare_manifest()?;
        let source_dir = package.join("src");
        let mut lib = String::new();
        for (index, rust) in generated.into_iter().enumerate() {
            fs::write(source_dir.join(format!("scope_{index}.rs")), rust)
                .map_err(|e| e.to_string())?;
            lib.push_str(&format!("pub mod scope_{index};\n"));
        }
        lib.push_str("pub fn named_graph_definitions() -> Vec<blkit::named_runtime::GraphDefinition> {\n    let mut definitions = Vec::new();\n");
        for (index, program) in programs.iter().enumerate() {
            if program.processes.iter().any(|p| p.named_graph.is_some()) {
                lib.push_str(&format!(
                    "    definitions.extend(scope_{index}::named_graph_definitions());\n"
                ));
            }
        }
        lib.push_str("    definitions\n}\n");
        fs::write(source_dir.join("lib.rs"), lib).map_err(|e| e.to_string())?;
        let bin_dir = source_dir.join("bin");
        fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&bin_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && (name.ends_with("-worker.rs") || name.ends_with("-server.rs"))
            {
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
        }
        let source = match self.build_target.as_str() {
            "worker" => Some(WORKER_SOURCE),
            "server" => Some(SERVER_SOURCE),
            _ => None,
        };
        if let Some(source) = source {
            fs::write(
                bin_dir.join(format!("{}-{}.rs", self.name, self.build_target)),
                source.replace("PROJECT_CRATE", &self.name.replace('-', "_")),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(package)
    }

    fn prepare_manifest(&self) -> Result<PathBuf, String> {
        let package = self.root.join(".blkit");
        let source_dir = package.join("src");
        fs::create_dir_all(&source_dir).map_err(|e| e.to_string())?;
        fs::write(package.join(".gitignore"), "*\n!.gitignore\n!Cargo.lock\n")
            .map_err(|e| e.to_string())?;
        let local = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dependency = if local.join("Cargo.toml").exists() {
            format!(
                "{{ version = \"={}\", path = {:?} }}",
                env!("CARGO_PKG_VERSION"),
                local.to_str().ok_or("non-UTF-8 blkit source path")?
            )
        } else {
            format!("\"={}\"", env!("CARGO_PKG_VERSION"))
        };
        let mut manifest = format!(
            "[package]\nname = {:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {dependency}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"time\", \"net\"] }}\naxum = \"0.8\"\n",
            self.name
        );
        for (name, dependency) in &self.dependencies {
            if name == "blkit"
                || matches!(
                    name.as_str(),
                    "rust_decimal" | "chrono" | "serde" | "serde_json" | "tokio" | "axum"
                )
            {
                return Err(format!("dependency {name} conflicts with generated crate"));
            }
            if let Some(path) = &dependency.path {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    Path::new("..").join(path)
                };
                manifest.push_str(&format!(
                    "{name} = {{ version = {:?}, path = {:?} }}\n",
                    dependency.version,
                    path.to_str().ok_or("non-UTF-8 dependency path")?
                ));
            } else {
                manifest.push_str(&format!("{name} = {:?}\n", dependency.version));
            }
        }
        fs::write(package.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
        if !source_dir.join("lib.rs").exists() {
            fs::write(source_dir.join("lib.rs"), "").map_err(|e| e.to_string())?;
        }
        Ok(package)
    }
}

const WORKER_SOURCE: &str = r#"use std::{env, time::Duration};
use blkit::{distributed::DistributedWorker, logging::{self, tracing}, postgres_store::PostgresStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!("usage: worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]");
        return Ok(());
    }
    let _logging = logging::init(concat!(env!("CARGO_PKG_NAME"), "-worker"))
        .map_err(|e| { eprintln!("logging configuration: {e}"); e })?;
    if let Err(reason) = run(&args).await {
        tracing::error!(reason, "worker error");
        return Err(reason.into());
    }
    Ok(())
}

async fn run(args: &[String]) -> Result<(), &'static str> {
    let url = args.get(1).ok_or("usage: worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]")?;
    if args.len() > 4 { return Err("usage: worker POSTGRES_URL [MAX_TASKS] [LEASE_MS]"); }
    let limit = args.get(2).map_or(Ok(32), |n| n.parse::<usize>()).map_err(|_| "invalid MAX_TASKS")?;
    let lease_ms = args.get(3).map_or(Ok(5000), |n| n.parse::<i64>()).map_err(|_| "invalid LEASE_MS")?;
    let store = PostgresStore::connect(url).await.map_err(|_| "cannot connect to postgres")?;
    let id = format!("worker-{}", std::process::id());
    let worker = DistributedWorker::new(store, &id, PROJECT_CRATE::named_graph_definitions(), limit, lease_ms)
        .map_err(|_| "invalid worker configuration")?;
    worker.advertise().await.map_err(|_| "cannot register worker")?;
    tracing::info!(worker_id = %id, "worker ready");
    loop {
        worker.run_once().await.map_err(|_| "worker runtime failure")?;
        if worker.drain_if_requested().await.map_err(|_| "cannot drain worker")? { return Ok(()); }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
"#;

const SERVER_SOURCE: &str = r#"use std::{env, path::Path, sync::Arc};
use blkit::{logging::{self, tracing}, runtime::{Engine, Registry, Store}, server::router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!("usage: server [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]\nDefault: blkit.db 32 127.0.0.1:3000");
        return Ok(());
    }
    let _logging = logging::init(concat!(env!("CARGO_PKG_NAME"), "-server"))
        .map_err(|e| { eprintln!("logging configuration: {e}"); e })?;
    if let Err(reason) = run(&args).await {
        tracing::error!(reason, "server error");
        return Err(reason.into());
    }
    Ok(())
}

async fn run(args: &[String]) -> Result<(), &'static str> {
    if args.len() > 4 { return Err("usage: server [DATABASE_FILE] [MAX_TASKS] [BIND_ADDRESS]"); }
    let database = args.get(1).map_or("blkit.db", String::as_str);
    let limit = args.get(2).map_or(Ok(32), |value| value.parse::<usize>()).map_err(|_| "invalid MAX_TASKS")?;
    let bind = args.get(3).map_or("127.0.0.1:3000", String::as_str);
    let store = Store::open(Path::new(database)).await.map_err(|_| "cannot open store")?;
    let registry = Registry::new_named(PROJECT_CRATE::named_graph_definitions()).map_err(|_| "invalid process definitions")?;
    let engine = Arc::new(Engine::new(registry, store, limit).map_err(|_| "invalid server configuration")?);
    engine.recover().await.map_err(|_| "cannot recover instances")?;
    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|_| "cannot bind server address")?;
    let address = listener.local_addr().map_err(|_| "cannot read bound address")?;
    tracing::info!(%address, "server listening");
    axum::serve(listener, router(engine)).await.map_err(|_| "server listener failed")?;
    Ok(())
}
"#;

fn discover(dir: &Path, sources: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            discover(&entry.path(), sources)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "bl") {
            sources.push(entry.path());
        }
    }
    Ok(())
}
