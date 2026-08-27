use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

#[test]
fn real_source_command_inspects_without_modifying_the_source() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let source = temporary.path().join("track.flac");
    fs::copy(fixture("seed.flac"), &source).expect("fixture should be copied");
    let before = fs::read(&source).expect("fixture should be readable");

    let output = Command::new(binary())
        .arg(&source)
        .stdin(Stdio::null())
        .output()
        .expect("music-groomer should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.contains("source inspection"));
    assert!(stdout.contains("one loose audio file; sibling files excluded"));
    assert!(stdout.contains("no files were changed"));
    assert!(stdout.contains("No provider was contacted and no destination was accessed"));
    assert_eq!(
        fs::read(&source).expect("source should remain readable"),
        before
    );
}

#[test]
fn blocking_inspection_returns_failure_after_explaining_it() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let source = temporary.path().join("broken.flac");
    fs::write(&source, "not audio").expect("broken fixture should be created");

    let output = Command::new(binary())
        .arg(&source)
        .stdin(Stdio::null())
        .output()
        .expect("music-groomer should run");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(stdout.contains("Blocker [broken.flac]"));
    assert!(stdout.contains("source remains untouched"));
    assert!(stderr.contains("inspection found blocking problems"));
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_music-groomer"))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}
