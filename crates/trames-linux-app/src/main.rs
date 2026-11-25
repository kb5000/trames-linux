// SPDX-License-Identifier: GPL-3.0-or-later
// Shared namespace resources form a recursive Send graph, as in the Mac product.
#![recursion_limit = "512"]
use std::{process::ExitCode, time::Duration};
use trames_kvm::{Doctor, ProbeOptions};
mod display;
mod foreground;
mod store;

const RUNTIME: &str = include_str!("../../../runtime-version.json");
const HELP: &str = "Trames Linux 0.1.0\n\
Usage:\n  trames doctor [--json] [--require-kvm]\n  \
trames probe [--json] [--transport auto|smccc|mmio] [--iterations N] [--deadline-ms N]\n\
  trames run [--cpus N] [--timeout-ms N] [--rootfs DIR] [--store DIR] [--audio] [--audio-input] [--trace-syscalls] [--network off|loopback|outbound|full] -- ELF [ARG...]\n\
The probe checks KVM execution and host requirements.\n\
  trames shell --rootfs DIR [--cpus N] [--timeout-ms N]\n\
  trames exec [--rootfs DIR] [--setenv K=V] [--cwd DIR] [-t] -- ELF [ARG...]\n\
  trames exec --socket PATH [-t] [--setenv K=V] -- COMMAND [ARG...]\n\
  trames session ls|attach --socket PATH [ID|--json|-q]\n\
Use read-file --store DIR -- /GUEST/PATH to export a stopped store file.\nUse --store DIR --rootfs DIR_OR_ARCHIVE to create a private VFS; use --store DIR to reopen it.\nRun an ELF program or open a shell in an explicit root filesystem. Enable PCM output with --audio and capture with --audio-input. Use --vulkan for headless GPU APIs, --vulkan-icd FILE to install the guest ICD, or --display x11 [--display-size WIDTHxHEIGHT] for a native fixed Vulkan display. Desktop composition and video remain under development.\n";

fn run() -> Result<u8, Box<dyn std::error::Error>> {
    let (selected, args) =
        trames_cli_common::arguments::selection(std::env::args().skip(1).collect())?;
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_else(|| "help".into());
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        print!("{HELP}");
        return Ok(0);
    }
    if command == "--version" {
        println!("trames-linux {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    if selected.is_some() {
        return Err(
            "named environments are not configured; use --rootfs DIR or --socket PATH".into(),
        );
    }
    if command == "read-file" {
        let args: Vec<_> = args.collect();
        if args.len() != 4 || args[0] != "--store" || args[2] != "--" || !args[3].starts_with('/') {
            return Err("usage: trames read-file --store DIR -- /GUEST/PATH".into());
        }
        use std::io::Write;
        let mut vfs = store::open(std::path::Path::new(&args[1]), None)?;
        let fd = vfs.open_at(trames_vfs::AT_FDCWD, args[3].as_bytes(), 0, 0)?;
        let mut output = std::io::stdout().lock();
        loop {
            let bytes = vfs.read(fd, 65536)?;
            if bytes.is_empty() {
                break;
            }
            output.write_all(&bytes)?;
        }
        return Ok(0);
    }
    if command == "exec" {
        let args: Vec<_> = args.collect();
        if args.first().is_some_and(|a| a == "--socket") {
            let socket = args.get(1).ok_or("missing exec socket")?;
            let request = trames_cli_common::arguments::parse_exec(&args[2..], false)?;
            return Ok(trames_cli_common::exec::run(
                std::path::Path::new(socket),
                request,
            )?);
        }
        return foreground::run(args, false);
    }
    if command == "run" || command == "shell" {
        return foreground::run(args.collect(), command == "shell");
    }
    if command == "session" {
        let args: Vec<_> = args.collect();
        let op = args.first().map(String::as_str).unwrap_or("ls");
        if args.get(1).is_none_or(|a| a != "--socket") {
            return Err("usage: trames session ls|attach --socket PATH [ID|--json|-q]".into());
        }
        let socket = std::path::Path::new(args.get(2).ok_or("missing session socket")?);
        match op {
            "ls" | "list" => trames_cli_common::console::list(socket, &args[3..])?,
            "attach" if args.len() == 4 => {
                trames_cli_common::console::attach(socket, args[3].parse()?)?
            }
            _ => return Err("usage: trames session ls|attach --socket PATH [ID|--json|-q]".into()),
        }
        return Ok(0);
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
    Ok(0)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("trames: {error}");
            ExitCode::from(2)
        }
    }
}
