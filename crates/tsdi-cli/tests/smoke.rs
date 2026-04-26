//! End-to-end smoke tests for the `tsdi` CLI binary.
//!
//! Validates only the M0 surface: `--version` prints, unknown subcommands
//! fail loudly, and the bare invocation exits successfully.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn prints_version() {
    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_exits_success() {
    Command::cargo_bin("tsdi").unwrap().assert().success();
}

#[test]
fn watch_subcommand_reports_not_yet_implemented() {
    Command::cargo_bin("tsdi")
        .unwrap()
        .arg("watch")
        .assert()
        .failure()
        .stderr(contains("not yet implemented"));
}
