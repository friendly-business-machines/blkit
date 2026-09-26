#![allow(dead_code)]

#[path = "src/expr.rs"] mod expr;
#[path = "src/graph.rs"] mod graph;
#[path = "src/semantic.rs"] mod semantic;
#[path = "src/codegen.rs"] mod codegen;
#[path = "src/compiler.rs"] mod compiler;
use compiler::{Program, Type, identifier, type_ref};

fn main() {
    for file in ["examples/graph.bl", "src/compiler.rs", "src/expr.rs", "src/graph.rs", "src/semantic.rs", "src/codegen.rs"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let source = std::fs::read_to_string("examples/graph.bl").expect("read example graph");
    let generated = compiler::transpile(&source).expect("compile example graph");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory")).join("graph.rs");
    std::fs::write(output, generated).expect("write generated graph");
}
