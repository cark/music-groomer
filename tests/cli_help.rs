use std::path::PathBuf;
use std::process::Command;

#[test]
fn top_level_help_documents_the_primary_workflow_and_hides_the_demo() {
    for help_argument in ["-h", "--help"] {
        let output = Command::new(binary()).arg(help_argument).output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage:"));
        assert!(stdout.contains("[SOURCE]"));
        assert!(stdout.contains("--offline"));
        assert!(stdout.contains("--cache-dir <DIRECTORY>"));
        assert!(stdout.contains("cache"));
        assert!(!stdout.contains("demo"));
    }
}

#[test]
fn cache_help_documents_status_clear_and_the_global_override() {
    let output = Command::new(binary())
        .args(["cache", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("status"));
    assert!(stdout.contains("clear"));
    assert!(stdout.contains("--cache-dir <DIRECTORY>"));
}

#[test]
fn unknown_switch_gets_a_conventional_cli_error() {
    let output = Command::new(binary())
        .arg("--definitely-unknown")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unexpected argument '--definitely-unknown'"));
    assert!(stderr.contains("--help"));
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_music-groomer"))
}
