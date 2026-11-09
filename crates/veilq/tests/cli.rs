//! The binary end to end: `redact --mapping` and `reveal` (ADR-0018), and the
//! MCP server over stdio (ADR-0016).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use age::secrecy::ExposeSecret;

const KEY: &str = "0123456789abcdef0123456789abcdef";

fn veilq(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_veilq"));
    c.current_dir(dir).env("VEILQ_TEST_KEY", KEY);
    c
}

fn policy(dir: &Path, recipient: Option<&str>) {
    let mut p = String::from(
        "version = 1\n[defaults]\naction = \"pseudonymize\"\n[entities.PERSON]\naction = \"fake\"\n\
         [pseudonymize]\nkey_env = \"VEILQ_TEST_KEY\"\n",
    );
    if let Some(r) = recipient {
        p.push_str(&format!("[mapping]\nrecipients = [\"{r}\"]\n"));
    }
    std::fs::write(dir.join("veilq.toml"), p).unwrap();
}

#[test]
fn redact_with_a_mapping_and_reveal() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let key = age::x25519::Identity::generate();
    policy(d, Some(&key.to_public().to_string()));
    std::fs::write(
        d.join("id.txt"),
        format!("{}\n", key.to_string().expose_secret()),
    )
    .unwrap();
    let input = "Hallo Jonas Brandt,\nRückruf unter 0421 5550173 oder mira.albers@example.com.\n";
    std::fs::write(d.join("in.txt"), input).unwrap();

    let out = veilq(d)
        .args([
            "redact",
            "in.txt",
            "-o",
            "out.txt",
            "--mapping",
            "out.map.age",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let redacted = std::fs::read_to_string(d.join("out.txt")).unwrap();
    assert!(
        redacted.contains("[PHONE_NUMBER:") && !redacted.contains("Brandt"),
        "{redacted}"
    );
    let map = std::fs::read(d.join("out.map.age")).unwrap();
    assert!(
        !String::from_utf8_lossy(&map).contains("Brandt"),
        "the mapping is encrypted"
    );

    let out = veilq(d)
        .args([
            "reveal",
            "out.txt",
            "--mapping",
            "out.map.age",
            "--identity",
            "id.txt",
            "-o",
            "back.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(d.join("back.txt")).unwrap(), input);

    // Another key reads nothing.
    let other = age::x25519::Identity::generate();
    std::fs::write(
        d.join("other.txt"),
        format!("{}\n", other.to_string().expose_secret()),
    )
    .unwrap();
    let out = veilq(d)
        .args([
            "reveal",
            "out.txt",
            "--mapping",
            "out.map.age",
            "--identity",
            "other.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn no_mapping_without_recipients_and_no_output_either() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    policy(d, None);
    std::fs::write(d.join("in.txt"), "Tel. 0421 5550173\n").unwrap();
    let out = veilq(d)
        .args(["redact", "in.txt", "-o", "out.txt", "--mapping", "m.age"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("recipients"));
    assert!(!d.join("out.txt").exists() && !d.join("m.age").exists());
}

#[test]
fn the_mcp_server_answers_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join("k.csv"), "name,telefon\nMira Albers,0421 5550173\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_veilq"))
        .current_dir(d)
        .args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut send = |v: serde_json::Value| writeln!(stdin, "{v}").unwrap();
    let answer = |lines: &mut std::io::Lines<_>| -> serde_json::Value {
        let l: String = lines.next().unwrap().unwrap();
        serde_json::from_str(&l).unwrap()
    };
    send(
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
    );
    let init = answer(&mut lines);
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    send(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    send(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = answer(&mut lines);
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for t in ["scan", "redact_text", "redact_file", "list_detectors"] {
        assert!(names.contains(&t), "{names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("reveal")));
    send(
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"redact_file","arguments":{"path":"k.csv"}}}),
    );
    let r = answer(&mut lines);
    assert_eq!(
        r["result"]["structuredContent"]["output"], "name,telefon\n<PERSON>,<PHONE_NUMBER>\n",
        "{r}"
    );
    drop(stdin);
    let _ = child.wait();
}
