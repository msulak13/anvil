//! End-to-end smoke tests for the `anvil` CLI binary.
//!
//! Validates only the M0 surface: `--version` prints, unknown subcommands
//! fail loudly, and the bare invocation exits successfully.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn prints_version() {
    Command::cargo_bin("anvil")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_exits_success() {
    Command::cargo_bin("anvil").unwrap().assert().success();
}

#[test]
fn watch_without_entry_or_config_errors() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("anvil")
        .unwrap()
        .current_dir(tmp.path())
        .arg("watch")
        .assert()
        .failure()
        .stderr(contains("no --entry or --config"));
}
