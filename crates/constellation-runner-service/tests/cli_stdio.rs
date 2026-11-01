//! Exercises the actual CLI process and byte framing, without opening sockets
//! or executing model-selected tools. The only secret is a generated test key.
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    process::{Command, Stdio},
};

use constellation_runner_protocol::{
    read_frame, write_frame, Capability, Endpoint, MessagePolicy, Pair, PermissionScope, Secret,
    Side,
};
use constellation_runner_service::SystemClock;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_constellation-runner-service")
}

fn command(args: &[&str]) -> std::process::Output {
    Command::new(binary()).args(args).output().unwrap()
}

fn key_bytes(path: &Path) -> [u8; 32] {
    let text = fs::read_to_string(path).unwrap();
    let text = text.trim();
    assert_eq!(text.len(), 64);
    let mut bytes = [0_u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap();
    }
    bytes
}

#[test]
fn real_stdio_runner_cli_requires_local_approval_and_returns_authenticated_receipt() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("local-runner.sqlite");
    let keyfile = root.path().join("private-key");
    let db = db.to_str().unwrap();
    let key = keyfile.to_str().unwrap();

    let generated = command(&["generate-key", key]);
    assert!(generated.status.success());
    assert!(!String::from_utf8_lossy(&generated.stdout).contains("private-key-content="));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&keyfile).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    assert!(!command(&["generate-key", key]).status.success()); // create_new only
    assert!(command(&["init", db, key, "workstation-1", "runner-1"])
        .status
        .success());
    assert!(!command(&["init", db, key, "workstation-1", "runner-1"])
        .status
        .success());
    assert!(!command(&["serve-stdio", db, key]).status.success()); // pending
    assert!(command(&["approve-pair", db, key]).status.success());
    assert!(!command(&["approve-pair", db, key]).status.success());

    let mut pair = Pair::enroll(
        "workstation-1",
        "runner-1",
        Secret::from_bytes(key_bytes(&keyfile)).unwrap(),
    )
    .unwrap();
    pair.approve().unwrap();
    let mut workstation = Endpoint::new(Side::Workstation, pair, MessagePolicy::default()).unwrap();
    let scope = PermissionScope {
        capability: Capability::DraftText,
        resource_id: "source-1".into(),
    };
    let clock = SystemClock;
    let signed = workstation
        .issue_job(
            "job-1",
            "agent-1",
            scope.clone(),
            "Only a queued draft, no tool",
            30_000,
            &clock,
        )
        .unwrap();

    let mut child = Command::new(binary())
        .args(["serve-stdio", db, key])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let input = child.stdin.as_mut().unwrap();
        write_frame(input, &signed).unwrap();
        input.flush().unwrap();
    }
    // EOF tells the bounded service to exit after its authenticated receipt.
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut reader = Cursor::new(output.stdout);
    let receipt = read_frame(&mut reader).unwrap().unwrap();
    assert!(workstation.receive(&receipt, &clock).unwrap().is_new());
    assert_eq!(read_frame(&mut reader).unwrap(), None);
    let list = command(&["list", db, key]);
    assert!(list.status.success());
    let rendered = String::from_utf8(list.stdout).unwrap();
    assert!(rendered.contains("job=job-1"));
    assert!(rendered.contains("state=Pending"));
    assert!(!rendered.contains("Only a queued draft"));

    assert!(
        !command(&["grant", db, key, "job-1", "draft_text", "wrong"])
            .status
            .success()
    );
    assert!(
        command(&["grant", db, key, "job-1", "draft_text", "source-1"])
            .status
            .success()
    );
    assert!(command(&["revoke-grant", db, key, "job-1"])
        .status
        .success());
    assert!(command(&["revoke-pair", db, key]).status.success());
    assert!(!command(&["serve-stdio", db, key]).status.success());
}
