use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tempfile::TempDir;

#[test]
fn cache_status_is_read_only_when_cache_does_not_exist() {
    let temporary = TempDir::new().unwrap();
    let cache_home = temporary.path().join("cache-home");

    let output = Command::new(binary())
        .arg("cache")
        .env("XDG_CACHE_HOME", &cache_home)
        .env("XDG_CONFIG_HOME", temporary.path().join("config-home"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("256.0 MiB"));
    assert!(stdout.contains("Metadata: 0 fresh, 0 stale"));
    assert!(!cache_home.exists());
}

#[test]
fn cache_clear_names_and_removes_only_the_marked_application_cache() {
    let temporary = TempDir::new().unwrap();
    let cache_home = temporary.path().join("cache-home");
    let application_cache = cache_home.join("music-groomer");
    fs::create_dir_all(application_cache.join("metadata")).unwrap();
    fs::write(
        application_cache.join(".music-groomer-cache"),
        "music-groomer provider cache\n",
    )
    .unwrap();
    fs::write(application_cache.join("metadata/broken.json"), "broken").unwrap();

    let mut child = Command::new(binary())
        .args(["cache", "clear"])
        .env("XDG_CACHE_HOME", &cache_home)
        .env("XDG_CONFIG_HOME", temporary.path().join("config-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write as _;
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(application_cache.to_str().unwrap()));
    assert!(stdout.contains("Provider cache cleared"));
    assert!(!application_cache.exists());
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_music-groomer"))
}
