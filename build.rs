#![allow(dead_code)]

#[path = "src/codegen/mod.rs"]
mod codegen;
#[path = "src/compiler.rs"]
mod compiler;
#[path = "src/decision.rs"]
mod decision;
#[path = "src/expr.rs"]
mod expr;
#[path = "src/graph.rs"]
mod graph;
pub use blkit_core::number_ops;
#[path = "src/semantic/mod.rs"]
mod semantic;
pub use blkit_core::temporal;
use compiler::{Program, Type, identifier, type_ref};

fn main() {
    for file in [
        "examples/graph.bl",
        "src/compiler.rs",
        "src/decision.rs",
        "src/expr.rs",
        "src/graph.rs",
        "crates/blkit-core/src/number_ops.rs",
        "crates/blkit-core/src/temporal.rs",
        "crates/blkit-core/src/temporal/duration.rs",
        "crates/blkit-core/src/temporal/calendar.rs",
        "crates/blkit-core/src/temporal/business.rs",
        "crates/blkit-core/src/temporal/financial.rs",
        "src/semantic/mod.rs",
        "src/semantic/decision.rs",
        "src/semantic/graph.rs",
        "src/semantic/types.rs",
        "src/codegen/mod.rs",
        "src/codegen/decision.rs",
        "src/codegen/graph.rs",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let source = std::fs::read_to_string("examples/graph.bl").expect("read example graph");
    let generated = compiler::transpile(&source).expect("compile example graph");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("graph.rs");
    std::fs::write(output, generated).expect("write generated graph");
}
