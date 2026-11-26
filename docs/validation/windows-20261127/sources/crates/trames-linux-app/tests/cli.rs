// SPDX-License-Identifier: GPL-3.0-or-later
use std::process::Command;
#[test]
fn doctor_reports_host_and_runtime_without_claiming_a_hardware_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_trames"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["host"]["host_os"], std::env::consts::OS);
    assert_eq!(value["host"]["host_arch"], std::env::consts::ARCH);
    assert_eq!(value["runtime"]["commit"].as_str().unwrap().len(), 40);
    assert!(value.get("probe").is_none());
}
#[test]
fn invalid_probe_requests_fail_without_a_success_report() {
    for args in [
        ["--transport", "rewrite"],
        ["--iterations", "1"],
        ["--deadline-ms", "0"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_trames"))
            .arg("probe")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn shared_exec_client_keeps_large_binary_stdin_and_separate_output_streams() {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
        process::Stdio,
    };
    let console = std::env::temp_dir().join(format!("trames-linux-cli-{}", std::process::id()));
    let socket = console.with_extension("exec");
    let listener = UnixListener::bind(&socket).unwrap();
    let payload: Vec<u8> = (0..200000).map(|n| (n % 256) as u8).collect();
    let expected = payload.clone();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut h = [0; 12];
        peer.read_exact(&mut h).unwrap();
        assert_eq!(&h[..8], b"TRMEXEC1");
        let mut request = vec![0; u32::from_be_bytes(h[8..].try_into().unwrap()) as usize];
        peer.read_exact(&mut request).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(
            request["argv"],
            serde_json::json!(["literal", "a b", "$(id)"])
        );
        let mut input = vec![];
        loop {
            let mut h = [0; 5];
            peer.read_exact(&mut h).unwrap();
            let mut body = vec![0; u32::from_be_bytes(h[1..].try_into().unwrap()) as usize];
            peer.read_exact(&mut body).unwrap();
            match h[0] {
                0 => input.extend(body),
                1 => break,
                _ => panic!("unexpected frame"),
            }
        }
        assert_eq!(input, expected);
        for (kind, body) in [
            (1, &[0, 255, 13, 10][..]),
            (2, &b"stderr"[..]),
            (3, &(7i32 << 8).to_be_bytes()[..]),
        ] {
            peer.write_all(&[kind]).unwrap();
            peer.write_all(&(body.len() as u32).to_be_bytes()).unwrap();
            peer.write_all(body).unwrap();
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_trames"))
        .args(["exec", "--socket"])
        .arg(&console)
        .args(["--", "literal", "a b", "$(id)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&payload).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    server.join().unwrap();
    std::fs::remove_file(socket).unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, [0, 255, 13, 10]);
    assert_eq!(output.stderr, b"stderr");
}
