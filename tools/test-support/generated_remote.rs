//! Test-owned generated remote packages; nested builds inherit Cargo target/profile controls.
use blkit_transpiler::project::Project;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Generated {
    pub root: PathBuf,
    package: PathBuf,
    pub name: String,
    pub build_target: &'static str,
}

impl Generated {
    pub fn new(target: &'static str) -> Self {
        Self::with_source(
            target,
            "namespace orders; version \"1\"; start_event start { output value: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.value -> done.result; }",
        )
    }

    pub fn with_source(target: &'static str, source: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "remote_{}_{}_{}",
            target.replace('-', "_"),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(&name);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("blkit.toml"),
            format!(
                "[project]\nname = {name:?}\nblkit = \"0.1.0\"\nbuild_target = {target:?}\n{}",
                if target == "api-worker" {
                    "persistence = \"remote\"\n"
                } else {
                    ""
                }
            ),
        )
        .unwrap();
        if target != "api-only" {
            fs::write(root.join("route.bl"), source).unwrap();
        }
        Project::load(&root).unwrap().transpile().unwrap();
        let package = root.join(".blkit");
        Self {
            root,
            package,
            name,
            build_target: target,
        }
    }

    pub fn build(&self, api_member: bool) -> PathBuf {
        let package = if api_member {
            self.package.join("api")
        } else {
            self.package.clone()
        };
        let output = Command::new("cargo")
            .args(["build", "--offline", "--manifest-path"])
            .arg(package.join("Cargo.toml"))
            .output()
            .unwrap();
        let log = std::env::temp_dir().join(format!("{}-build-{}.log", self.name, api_member));
        fs::write(&log, [&output.stdout[..], &output.stderr[..]].concat()).unwrap();
        assert!(
            output.status.success(),
            "generated build failed; log: {}",
            log.display()
        );
        let tree = Command::new("cargo")
            .args([
                "tree",
                "--offline",
                "--edges",
                "normal",
                "--prefix",
                "none",
                "--manifest-path",
            ])
            .arg(package.join("Cargo.toml"))
            .output()
            .unwrap();
        assert!(tree.status.success());
        let tree = String::from_utf8(tree.stdout).unwrap();
        let tree_log = log.with_extension("tree.log");
        fs::write(&tree_log, &tree).unwrap();
        for forbidden in ["blkit-cli", "blkit-transpiler", "chumsky", "logos", "turso"] {
            assert!(
                !tree
                    .lines()
                    .any(|line| line.starts_with(&format!("{forbidden} v"))),
                "unexpected {forbidden}; {}",
                tree_log.display()
            );
        }
        let api_only = api_member || self.build_target == "api-only";
        let combined = self.build_target == "api-worker";
        if !api_only && !combined {
            assert!(
                !tree.lines().any(|line| line.starts_with("axum v")),
                "worker acquired HTTP server dependencies"
            );
        }
        if api_only {
            assert!(
                !package.join("src/lib.rs").exists(),
                "API links generated definitions"
            );
            assert!(
                !fs::read_to_string(package.join("Cargo.toml"))
                    .unwrap()
                    .contains("\"worker\"")
            );
            assert!(
                !tree.lines().any(|line| line.starts_with("opentelemetry")),
                "API acquired worker logging dependencies"
            );
        }
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.package.join("target"));
        target.join("debug")
    }
}

impl Drop for Generated {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
