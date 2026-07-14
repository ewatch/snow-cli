use super::*;
use crate::snu::broker::InstanceSummary;
use crate::snu::protocol::normalize_origin;
use std::io::Cursor;

fn instance_summary(url: &str, is_latest: bool) -> InstanceSummary {
    InstanceSummary {
        url: url.to_string(),
        origin: normalize_origin(url).unwrap(),
        has_g_ck: true,
        scope: Some("global".to_string()),
        is_latest,
        captured_at: Some(1_700_000_000),
        last_verified_at: None,
    }
}

fn broker_status(instances: Vec<InstanceSummary>, browser_connected: bool) -> BrokerStatus {
    let latest = instances
        .iter()
        .find(|i| i.is_latest)
        .map(|i| i.url.clone());
    BrokerStatus {
        version: "0.0.0-test".to_string(),
        ipc_addr: "127.0.0.1:1979".to_string(),
        browser_connected,
        session_count: instances.len(),
        latest_instance_url: latest,
        instances,
        idle_timeout_secs: 1800,
    }
}

#[test]
fn check_connection_result_reflects_probe_and_state() {
    let status = broker_status(
        vec![instance_summary("https://dev.service-now.com", true)],
        true,
    );
    let value = build_check_connection_result(&status, true, None);
    assert_eq!(value["connected"], true);
    assert_eq!(value["broker_running"], true);
    assert_eq!(value["browser_connected"], true);
    assert_eq!(value["session_count"], 1);
    assert_eq!(value["latest_instance_url"], "https://dev.service-now.com");
    assert_eq!(value["instances"].as_array().unwrap().len(), 1);
    assert!(value.get("token_valid").is_none());
}

#[test]
fn check_connection_result_marks_unresponsive_probe_disconnected() {
    let status = broker_status(Vec::new(), false);
    let value = build_check_connection_result(&status, false, None);
    assert_eq!(value["connected"], false);
    assert_eq!(value["session_count"], 0);
}

#[test]
fn check_connection_result_reports_token_validity() {
    let status = broker_status(
        vec![instance_summary("https://dev.service-now.com", true)],
        true,
    );
    let valid = build_check_connection_result(&status, true, Some(Ok(true)));
    assert_eq!(valid["token_valid"], true);
    assert!(valid.get("hint").is_none());

    let dead = build_check_connection_result(&status, true, Some(Ok(false)));
    assert_eq!(dead["token_valid"], false);
    assert!(
        dead["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("/token"))
    );

    let failed = build_check_connection_result(&status, true, Some(Err(anyhow!("no session"))));
    assert_eq!(failed["token_valid"], Value::Null);
    assert_eq!(failed["verify_error"], "no session");
}

#[test]
fn instance_info_result_defaults_to_latest_session() {
    let status = broker_status(
        vec![
            instance_summary("https://a.service-now.com", false),
            instance_summary("https://b.service-now.com", true),
        ],
        true,
    );
    let value = build_instance_info_result(&status, None).unwrap();
    assert_eq!(value["url"], "https://b.service-now.com");
    assert_eq!(value["has_g_ck"], true);
    assert_eq!(value["scope"], "global");
    assert_eq!(value["is_latest"], true);
}

#[test]
fn instance_info_result_selects_requested_origin() {
    let status = broker_status(
        vec![
            instance_summary("https://a.service-now.com", false),
            instance_summary("https://b.service-now.com", true),
        ],
        true,
    );
    let origin = normalize_origin("https://a.service-now.com").unwrap();
    let value = build_instance_info_result(&status, Some(&origin)).unwrap();
    assert_eq!(value["url"], "https://a.service-now.com");
}

#[test]
fn instance_info_result_errors_without_session() {
    let status = broker_status(Vec::new(), false);
    let err = build_instance_info_result(&status, None).unwrap_err();
    assert!(err.to_string().contains("Run /token"));
}

#[test]
fn instance_info_result_errors_for_unknown_origin() {
    let status = broker_status(
        vec![instance_summary("https://a.service-now.com", true)],
        true,
    );
    let err =
        build_instance_info_result(&status, Some("https://gone.service-now.com:443")).unwrap_err();
    assert!(err.to_string().contains("no SN-Utils browser session for"));
}

#[test]
fn query_string_encodes_fields_and_query() {
    let qs = build_table_query_string(
        "sys_id,number",
        5,
        Some("active=true"),
        Some("ORDERBYnumber"),
    );
    assert_eq!(
        qs,
        "sysparm_fields=sys_id%2Cnumber&sysparm_limit=5&sysparm_query=active%3Dtrue%5EORDERBYnumber"
    );
}

#[test]
fn resolve_script_from_code_takes_precedence() {
    let script = resolve_script_from(
        Some("file.js".into()),
        Some("gs.info('from code')".into()),
        Cursor::new(b"ignored"),
        false,
    )
    .unwrap();
    assert_eq!(script, "gs.info('from code')");
}

#[test]
fn resolve_script_from_stdin() {
    let script = resolve_script_from(None, None, Cursor::new(b"gs.info('stdin')"), false).unwrap();
    assert_eq!(script, "gs.info('stdin')");
}

#[test]
fn resolve_script_from_tty_no_input_errors() {
    let err = resolve_script_from(None, None, Cursor::new(b""), true).unwrap_err();
    assert!(err.to_string().contains("No script provided"));
}

fn fields_from(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

#[test]
fn js_json_literal_escapes_quotes_backslashes_and_newlines() {
    let value = Value::String("a\"b\\c\nd\tE".to_string());
    let literal = js_json_literal(&value);
    // Valid JSON (and thus a valid JS literal) that round-trips exactly.
    let back: Value = serde_json::from_str(&literal).unwrap();
    assert_eq!(back, value);
    assert!(
        !literal.contains('\n'),
        "raw newline must not leak: {literal}"
    );
}

#[test]
fn js_json_literal_escapes_js_line_separators() {
    let value = Value::String("a\u{2028}b\u{2029}c".to_string());
    let literal = js_json_literal(&value);
    assert!(!literal.contains('\u{2028}'));
    assert!(!literal.contains('\u{2029}'));
    assert!(literal.contains("\\u2028"));
    assert!(literal.contains("\\u2029"));
}

#[test]
fn update_script_embeds_fields_safely() {
    let fields = fields_from(&[
        (
            "short_description",
            Value::String("line1\nline2 \"q\"".into()),
        ),
        (
            "script",
            Value::String("gs.info('hi'); var x = \"</y>\";".into()),
        ),
        ("count", Value::from(3)),
        ("emoji", Value::String("héllo 🚀".into())),
    ]);
    let script = build_update_script("incident", "abc'123\\", &fields);
    // No raw newline from user data should appear outside the template lines.
    assert!(script.contains("new GlideRecord(__table)"));
    assert!(script.contains(MUTATION_RESULT_MARKER));
    assert!(script.contains("__gr.update()"));
    // The embedded fields literal must be valid JSON round-tripping the input.
    let start = script.find("var __fields = ").unwrap() + "var __fields = ".len();
    let rest = &script[start..];
    let end = rest.find(";\n").unwrap();
    let parsed: Value = serde_json::from_str(rest[..end].trim()).unwrap();
    assert_eq!(parsed, Value::Object(fields));
}

#[test]
fn delete_scripts_are_generated() {
    let single = build_delete_script("incident", "abc123");
    assert!(single.contains("__gr.deleteRecord()"));
    assert!(single.contains(MUTATION_RESULT_MARKER));

    let bulk = build_bulk_delete_script("incident", "active=true^ORDERBYnumber", 25);
    assert!(bulk.contains("addEncodedQuery(__query)"));
    assert!(bulk.contains("setLimit(__limit)"));
    assert!(bulk.contains("var __limit = 25;"));
}

#[test]
fn parse_mutation_result_extracts_marked_line() {
    let data = format!(
        "*** Script: running\n{MUTATION_RESULT_MARKER}{{\"success\":true,\"updated\":1}}\ndone\n"
    );
    let result = parse_mutation_result(&data).unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["updated"], 1);
}

#[test]
fn parse_mutation_result_uses_last_marker() {
    let data = format!(
        "{MUTATION_RESULT_MARKER}{{\"success\":false}}\n{MUTATION_RESULT_MARKER}{{\"success\":true}}"
    );
    let result = parse_mutation_result(&data).unwrap();
    assert_eq!(result["success"], true);
}

#[test]
fn parse_mutation_result_falls_back_to_whole_output() {
    let result = parse_mutation_result("  {\"success\":true,\"deleted\":1}  ").unwrap();
    assert_eq!(result["deleted"], 1);
}

#[test]
fn parse_mutation_result_errors_on_garbage() {
    assert!(parse_mutation_result("not json at all").is_err());
}

#[test]
fn parse_mutation_result_strips_html_wrapper() {
    // Live-observed shape: sys.scripts.do wraps the printed line in the
    // page's HTML, so the JSON is followed by <BR/> and closing tags.
    let data = format!(
        "*** Script: {MUTATION_RESULT_MARKER}{{\"success\":true,\"action\":\"update\",\"updated\":1}}<BR/></PRE><HR/></BODY></HTML>"
    );
    let value = parse_mutation_result(&data).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["updated"], 1);
}

#[test]
fn parse_mutation_result_decodes_html_entities() {
    let data = format!(
        "{MUTATION_RESULT_MARKER}{{&quot;success&quot;:true,&quot;action&quot;:&quot;delete&quot;,&quot;deleted&quot;:1}}<BR/>"
    );
    let value = parse_mutation_result(&data).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["deleted"], 1);
}

#[test]
fn parse_mutation_result_maps_empty_output_to_expired_session() {
    let error = parse_mutation_result("  \n ").unwrap_err().to_string();
    assert!(
        error.contains("/token"),
        "error should point at /token: {error}"
    );
    assert!(
        error.contains("expired"),
        "error should mention expiry: {error}"
    );
}

#[test]
fn parse_mutation_result_detects_login_page() {
    let data = "<html><head><title>Login</title></head><body><form><input name=\"user_name\"/></form></body></html>";
    let error = parse_mutation_result(data).unwrap_err().to_string();
    assert!(
        error.contains("/token"),
        "error should point at /token: {error}"
    );
    assert!(
        error.contains("logged out"),
        "error should say the session is logged out: {error}"
    );
}

#[test]
fn guesses_common_content_types() {
    assert_eq!(guess_content_type(Path::new("a.png")), "image/png");
    assert_eq!(
        guess_content_type(Path::new("a.unknown")),
        "application/octet-stream"
    );
}
