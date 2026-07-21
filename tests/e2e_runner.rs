#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_scenario(path: &Path, name: &str, command: &str, destructive: bool) {
    let destructive_metadata = if destructive {
        "session_destructive = true\n"
    } else {
        ""
    };
    fs::write(
        path,
        format!(
            r#"name = "{name}"
requires = ["none"]
{destructive_metadata}
[command]
args = ["{command}"]

[expect]
exit_code = 0
"#,
        ),
    )
    .unwrap();
}

fn run_runner(fixture: &Path, scenarios: &[PathBuf], extra_env: &[(&str, &str)]) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fake_cli = fixture.join("fake-snow-cli");
    let artifacts = fixture.join("artifacts");
    let execution_log = fixture.join("execution.log");
    let destroyed_state = fixture.join("session-destroyed");

    let mut command = Command::new("bash");
    command
        .arg(repo.join("scripts/e2e-run"))
        .args(scenarios)
        .current_dir(repo)
        .env("SNOW_CLI_BIN", fake_cli)
        .env("SNOW_E2E_ARTIFACTS_DIR", artifacts)
        .env("SNOW_E2E_EXECUTION_LOG", execution_log)
        .env("SNOW_E2E_DESTROYED_STATE", destroyed_state)
        .env("SNOW_E2E_SN_UTILS", "0")
        .env_remove("SNOW_E2E_INSTANCE_URL")
        .env_remove("SNOW_E2E_USERNAME")
        .env_remove("SNOW_E2E_PASSWORD");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "runner failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn session_destructive_scenarios_run_last_and_still_run_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let fake_cli = fixture.path().join("fake-snow-cli");
    write_executable(
        &fake_cli,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >> "$SNOW_E2E_EXECUTION_LOG"
case "$1" in
  shared)
    [[ ! -e "$SNOW_E2E_DESTROYED_STATE" ]] || exit 41
    ;;
  clear|stop)
    : > "$SNOW_E2E_DESTROYED_STATE"
    ;;
esac
printf '{"ok":true}\n'
"#,
    );

    let clear = fixture.path().join("clear.toml");
    let shared = fixture.path().join("shared.toml");
    let stop = fixture.path().join("stop.toml");
    write_scenario(&clear, "clear", "clear", true);
    write_scenario(&shared, "shared", "shared", false);
    write_scenario(&stop, "stop", "stop", true);
    fs::write(
        &clear,
        fs::read_to_string(&clear).unwrap() + "\n[[cleanup]]\nargs = [\"clear-cleanup\"]\n",
    )
    .unwrap();

    let output = run_runner(
        fixture.path(),
        &[clear.clone(), shared.clone(), stop.clone()],
        &[],
    );
    assert_success(&output);

    let execution_log = fs::read_to_string(fixture.path().join("execution.log")).unwrap();
    assert_eq!(execution_log, "shared\nclear\nclear-cleanup\nstop\n");

    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "tests/e2e/scenarios/snu/broker/clear.toml",
        "tests/e2e/scenarios/snu/broker/stop.toml",
    ] {
        let metadata = fs::read_to_string(repo.join(relative)).unwrap();
        assert!(
            metadata.contains("session_destructive = true"),
            "{relative} must declare its shared-session side effect"
        );
    }
}

#[test]
fn artifacts_redact_structured_and_text_session_tokens_everywhere() {
    let fixture = tempfile::tempdir().unwrap();
    let fake_cli = fixture.path().join("fake-snow-cli");
    write_executable(
        &fake_cli,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$SNOW_E2E_EXECUTION_LOG"
printf '{"instance":{"g_ck":"%s","sys_id":"safe-sys-id"},"echo":"%s","sessionToken":"%s","Authorization":"Bearer %s"}\n' \
  "$TEST_GCK" "$TEST_GCK" "$TEST_SESSION_TOKEN" "$TEST_AUTH_TOKEN"
printf 'g_ck=%s X-UserToken: %s repeated=%s\nAuthorization: Bearer %s\nProxy-Authorization: Basic %s\n' \
  "$TEST_TEXT_GCK" "$TEST_HEADER_TOKEN" "$TEST_GCK" "$TEST_AUTH_TOKEN" "$TEST_PROXY_TOKEN" >&2
"#,
    );

    let scenario = fixture.path().join("secrets.toml");
    fs::write(
        &scenario,
        r#"name = "artifact-secrets"
description = "Repeated token: $TEST_GCK"
requires = ["none"]

[command]
args = ["emit", "$TEST_GCK", "--g_ck=$TEST_ARGV_TOKEN"]

[expect]
exit_code = 0

[[fuzzy]]
expectation = "The generated session token $TEST_SESSION_TOKEN is never persisted."
"#,
    )
    .unwrap();

    let secrets = [
        ("TEST_GCK", "gck-structural-secret"),
        ("TEST_SESSION_TOKEN", "session-structural-secret"),
        ("TEST_TEXT_GCK", "gck-text-secret"),
        ("TEST_HEADER_TOKEN", "header-token-secret"),
        ("TEST_ARGV_TOKEN", "argv-token-secret"),
        ("TEST_AUTH_TOKEN", "authorization-token-secret"),
        ("TEST_PROXY_TOKEN", "proxy-authorization-secret"),
        ("SNOW_E2E_USERNAME", "admin"),
        ("SNOW_E2E_PASSWORD", "admin-TopSecret"),
        ("SNOW_E2E_INSTANCE_URL", "https://secret-instance.example"),
        ("SNOW_E2E_HARNESS", "g_ck=harness-metadata-secret"),
        ("SNOW_E2E_MODEL", "session_token=model-metadata-secret"),
    ];
    let output = run_runner(fixture.path(), &[scenario], &secrets);
    assert_success(&output);

    let artifact_path = fixture.path().join("artifacts/artifact-secrets.json");
    let result = fs::read_to_string(&artifact_path).unwrap_or_else(|error| {
        let entries = fs::read_dir(fixture.path().join("artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        panic!(
            "could not read {artifact_path:?}: {error}; entries: {entries:?}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let aggregate = fs::read_to_string(fixture.path().join("artifacts/results.jsonl")).unwrap();
    let summary = fs::read_to_string(fixture.path().join("artifacts/summary.md")).unwrap();

    for secret in secrets.map(|(_, value)| value) {
        assert!(
            !result.contains(secret),
            "secret remained in {artifact_path:?}"
        );
        assert!(
            !aggregate.contains(secret),
            "secret remained in results.jsonl"
        );
        assert!(!summary.contains(secret), "secret remained in summary.md");
    }
    assert!(result.contains("safe-sys-id"));
    assert!(result.contains("g_ck"));
    assert!(result.contains("sessionToken"));
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("TopSecret"));
    assert!(!aggregate.contains("TopSecret"));
    assert!(!summary.contains("TopSecret"));

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let stdout: serde_json::Value =
        serde_json::from_str(parsed["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(stdout["instance"]["g_ck"], "[REDACTED]");
    assert_eq!(stdout["instance"]["sys_id"], "safe-sys-id");
    assert_eq!(stdout["sessionToken"], "[REDACTED]");
    assert!(parsed["stderr"].as_str().unwrap().contains("[REDACTED]"));
    assert!(
        parsed["command"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .all(|arg| !arg.as_str().unwrap().contains("secret"))
    );
}

#[test]
fn unresolved_null_capture_fails_without_invoking_the_command_and_records_cleanup_warning() {
    let fixture = tempfile::tempdir().unwrap();
    let fake_cli = fixture.path().join("fake-snow-cli");
    write_executable(
        &fake_cli,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >> "$SNOW_E2E_EXECUTION_LOG"
printf '{"sys_id":null}\n'
"#,
    );

    let scenario = fixture.path().join("unresolved.toml");
    fs::write(
        &scenario,
        r#"name = "unresolved-capture"
requires = ["none"]

[[setup]]
args = ["seed"]
capture = { sys_id = ".sys_id" }

[command]
args = ["must-not-run", "{{sys_id}}"]

[[cleanup]]
args = ["cleanup-must-not-run", "{{sys_id}}"]

[expect]
exit_code = 0
"#,
    )
    .unwrap();

    let output = run_runner(fixture.path(), &[scenario], &[]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(fixture.path().join("execution.log")).unwrap(),
        "seed\n"
    );

    let result =
        fs::read_to_string(fixture.path().join("artifacts/unresolved-capture.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], "fail");
    assert_eq!(parsed["command_ran"], false);
    assert_eq!(parsed["command"]["exit_code"], serde_json::Value::Null);
    assert!(
        parsed["failure_reasons"][0]
            .as_str()
            .unwrap()
            .contains("command has unresolved capture placeholder {{sys_id}}")
    );
    assert!(
        parsed["cleanup_warnings"][0]
            .as_str()
            .unwrap()
            .contains("cleanup[0] has unresolved capture placeholder {{sys_id}}")
    );
}
