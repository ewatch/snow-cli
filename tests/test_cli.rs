mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ServiceNow"));
}

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("snow-cli"));
}

#[test]
fn test_no_args_shows_help() {
    cargo_bin_cmd!("snow-cli")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn test_config_show_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage configuration"));
}

#[test]
fn test_table_list_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("List records"));
}

#[test]
fn test_incident_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["incident", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Incident"));
}

#[test]
fn test_completions_bash() {
    cargo_bin_cmd!("snow-cli")
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("snow-cli"));
}

#[test]
fn test_invalid_subcommand() {
    cargo_bin_cmd!("snow-cli")
        .arg("nonexistent")
        .assert()
        .failure();
}
