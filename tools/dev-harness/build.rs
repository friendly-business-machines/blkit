fn main() {
    println!("cargo:rerun-if-changed=examples/graph.bl");
    let source = std::fs::read_to_string("examples/graph.bl").expect("read example graph");
    let generated = blkit_transpiler::transpile(&source).expect("compile example graph");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("graph.rs");
    std::fs::write(output, generated).expect("write generated graph");
}
