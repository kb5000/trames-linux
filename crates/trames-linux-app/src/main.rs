// SPDX-License-Identifier: GPL-3.0-or-later
use std::{process::ExitCode, time::Duration};
use trames_kvm::{Doctor, ProbeOptions};

const RUNTIME: &str = include_str!("../../../runtime-version.json");
const HELP: &str = "Trames Linux 0.1.0 — KVM bring-up\n\
Usage:\n  trames doctor [--json] [--require-kvm]\n  \
trames probe [--json] [--transport auto|smccc|mmio] [--iterations N] [--deadline-ms N]\n\
The probe runs original EL0 fixtures through a private EL1 adapter.\n\
General ELF/rootfs sessions and media providers are not implemented yet.\n";

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".into());
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if command == "--version" {
        println!("trames-linux {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !matches!(command.as_str(), "doctor" | "probe") {
        return Err(format!("unknown command {command:?}\n{HELP}").into());
    }
    let mut json = false;
    let mut require_kvm = false;
    let mut options = ProbeOptions::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--require-kvm" if command == "doctor" => require_kvm = true,
            "--transport" if command == "probe" => {
                options.transport = args.next().ok_or("missing transport")?.parse()?
            }
            "--iterations" if command == "probe" => {
                options.iterations = args.next().ok_or("missing iteration count")?.parse()?
            }
            "--deadline-ms" if command == "probe" => {
                options.deadline =
                    Duration::from_millis(args.next().ok_or("missing deadline")?.parse()?)
            }
            _ => return Err(format!("unexpected argument {arg:?}").into()),
        }
    }
    let runtime: serde_json::Value = serde_json::from_str(RUNTIME)?;
    if command == "doctor" {
        let host = Doctor::inspect();
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"runtime": runtime, "host": host})
                )?
            );
        } else {
            println!(
                "host: {}/{}; page size: {:?}; KVM API: {:?}; backend implemented: {}",
                host.host_os,
                host.host_arch,
                host.host_page_size,
                host.kvm_api_version,
                host.backend_implemented
            );
            for issue in &host.issues {
                println!("  {issue}");
            }
        }
        if require_kvm {
            host.require_probe()?;
        }
    } else {
        let result = trames_kvm::probe(&options)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"runtime": runtime, "probe": result})
                )?
            );
        } else {
            println!(
                "L0 KVM probe passed: {:?}, {} syscall round trips, {:.1} ns/round trip",
                result.transport, result.syscall_round_trips, result.ns_per_round_trip
            );
            println!("4 KiB permissions, canonical RAM, GPR/SIMD/TLS, fault resume and deadline verified");
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("trames: {error}");
            ExitCode::from(2)
        }
    }
}
