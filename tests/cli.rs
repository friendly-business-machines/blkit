use std::process::Command;

#[test]
fn help_explains_source_and_output() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("SOURCE.bl"));
    assert!(text.contains("OUTPUT.rs"));
}

#[test]
fn missing_arguments_fail_with_usage() {
    let result = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr).unwrap().contains("usage:"));
}

#[test]
fn valid_source_writes_rust() {
    let directory = std::env::temp_dir().join(format!("blkit-cli-valid-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.bl");
    let output = directory.join("output.rs");
    std::fs::write(&input, "namespace orders\nversion \"1.0\"\nprocess echo(input: Number) -> Number:\n  return input\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blkit")).args([&input, &output]).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(std::fs::read_to_string(&output).unwrap().contains("pub fn echo"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_source_produces_diagnostic_without_output() {
    for (case, source, error) in [
        ("parse", "namespace orders\n", "version"),
        ("type", "namespace orders\nversion \"1.0\"\nprocess echo(input: Unknown) -> Number:\n  return 1\n", "Unknown"),
    ] {
        let directory = std::env::temp_dir().join(format!("blkit-cli-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let input = directory.join("input.bl");
        let output = directory.join("output.rs");
        std::fs::write(&input, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blkit")).args([&input, &output]).output().unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(error));
        assert!(!output.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
