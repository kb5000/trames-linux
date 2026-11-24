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
