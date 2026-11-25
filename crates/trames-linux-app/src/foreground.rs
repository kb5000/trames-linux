// SPDX-License-Identifier: GPL-3.0-or-later
use std::{
    io::{self, Write},
    sync::Arc,
    time::Instant,
};
use trames_cli_common::{
    arguments::Foreground,
    signals::Signals,
    terminal::{window_size, RawTerminal},
};
use trames_linux_abi::{GuestOutput, OutputFd, TerminalEvent};
struct Output {
    source: Arc<dyn trames_vfs::ConsoleIo>,
    deadline: Option<Instant>,
    signals: Signals,
    pty: bool,
    window: Option<(u16, u16)>,
}
impl GuestOutput for Output {
    fn source_console(&mut self) -> Arc<dyn trames_vfs::ConsoleIo> {
        self.source.clone()
    }
    fn cancellation_requested(&self) -> bool {
        self.signals.stopping() || self.deadline.is_some_and(|d| Instant::now() >= d)
    }
    fn waits_for_host(&self) -> bool {
        true
    }
    fn host_interest(&mut self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
        wait.watch(&self.signals, false)?;
        if let Some(deadline) = self.deadline {
            wait.until(deadline);
        }
        Ok(())
    }
    fn poll_host(&mut self, _: &trames_vfs::Vfs) -> io::Result<trames_linux_abi::HostState> {
        self.signals.drain();
        Ok(trames_linux_abi::HostState::from_cancelled(
            self.cancellation_requested(),
        ))
    }
    fn stdin_ready(&mut self) -> io::Result<bool> {
        Ok(self.source.poll(trames_vfs::ConsoleStream::Input, 1).0 != 0)
    }
    fn poll_stdin(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if !self.stdin_ready()? {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let input = self.source.read(bytes.len()).map_err(|e| match e.0 {
            11 => io::Error::from(io::ErrorKind::WouldBlock),
            4 => io::Error::from(io::ErrorKind::Interrupted),
            32 => io::Error::from(io::ErrorKind::BrokenPipe),
            code => io::Error::from_raw_os_error(code as i32),
        })?;
        bytes[..input.len()].copy_from_slice(&input);
        Ok(input.len())
    }
    fn terminal_event(&mut self) -> Option<TerminalEvent> {
        if !self.pty {
            return None;
        }
        let size = window_size()?;
        if self.window == Some(size) {
            return None;
        }
        self.window = Some(size);
        Some(TerminalEvent::Resize {
            rows: size.0,
            columns: size.1,
        })
    }
    fn write(&mut self, fd: OutputFd, bytes: &[u8]) -> io::Result<usize> {
        match fd {
            OutputFd::Stdout => io::stdout().write(bytes),
            OutputFd::Stderr => io::stderr().write(bytes),
        }
    }
}
// Product capabilities stay in the product adapter, independent of shared CLI.
#[derive(Default)]
struct HostOptions {
    runtime: trames_session::RuntimeOptions,
    store: Option<std::path::PathBuf>,
    network: Option<trames_vfs::NetworkMode>,
    trace: bool,
}
fn host_options(args: Vec<String>) -> Result<(Vec<String>, HostOptions), String> {
    let mut out = vec![];
    let mut host = HostOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                out.push(arg);
                out.extend(args);
                break;
            }
            "--store" => {
                if host.store.is_some() {
                    return Err("duplicate --store".into());
                }
                let path = args
                    .next()
                    .filter(|p| p != "--")
                    .ok_or("missing --store directory")?;
                host.store = Some(path.into());
            }
            "--network" => {
                if host.network.is_some() {
                    return Err("duplicate --network".into());
                }
                host.network = Some(match args.next().as_deref() {
                    Some("off") => trames_vfs::NetworkMode::Off,
                    Some("loopback") => trames_vfs::NetworkMode::Loopback,
                    Some("outbound") => trames_vfs::NetworkMode::Outbound,
                    Some("full") => trames_vfs::NetworkMode::Full,
                    _ => return Err("--network requires off|loopback|outbound|full".into()),
                });
            }
            "--trace-syscalls" => host.trace = true,
            "--audio" => host.runtime.audio_output = true,
            "--audio-input" => host.runtime.audio_input = true,
            "--rootfs" | "--cpus" | "--timeout-ms" | "--setenv" | "--cwd" | "-w" => {
                out.push(arg);
                if let Some(value) = args.next() {
                    out.push(value);
                }
            }
            _ => out.push(arg),
        }
    }
    Ok((out, host))
}
pub fn run(args: Vec<String>, shell: bool) -> Result<u8, Box<dyn std::error::Error>> {
    use std::io::IsTerminal;
    let (args, host) = host_options(args)?;
    let store = host.store;
    let no_pty = args
        .iter()
        .take_while(|a| a.as_str() != "--")
        .any(|a| a == "--no-pty");
    let mut command = Foreground::parse(
        args,
        if shell {
            Some(&["/bin/sh", "-l"])
        } else {
            None
        },
    )?;
    if shell {
        if command.rootfs.is_none() && store.is_none() {
            return Err("shell requires --rootfs DIR or --store DIR".into());
        }
        command.request.tty |= !no_pty && io::stdin().is_terminal();
    }
    let program = command.request.argv.first().unwrap().clone();
    if (store.is_some() || command.rootfs.is_some()) && !program.starts_with('/') {
        return Err("with --rootfs or --store the program must be an absolute guest path".into());
    }
    // Probe before reading an ELF or mounting a host directory on unsupported hosts.
    trames_kvm::Doctor::inspect().require_probe()?;
    let mut vfs = if let Some(path) = &store {
        crate::store::open(path, command.rootfs.as_deref().map(std::path::Path::new))?
    } else {
        trames_vfs::Vfs::default()
    };
    if let Some(network) = host.network {
        vfs.configure_local_fabric(network)?;
    }
    // Seed pseudo inodes before attaching a rootfs, as in shared init boot.
    vfs.set_runtime_info(1, Default::default());
    let store_sync = store.as_ref().map(|_| vfs.host_control_view());
    let (bytes, guest_name) = if store.is_some() || command.rootfs.is_some() {
        if store.is_none() {
            // Retain the runtime pseudo mounts before entering the host grant.
            // Replacing / directly hides their original attachment points.
            vfs.mkdir_at(trames_vfs::AT_FDCWD, b"/guest", 0o755)?;
            vfs.mount_host(b"/guest", command.rootfs.as_ref().unwrap(), false)?;
            for name in ["proc", "sys", "dev"] {
                match vfs.mkdir_at(
                    trames_vfs::AT_FDCWD,
                    format!("/guest/{name}").as_bytes(),
                    0o755,
                ) {
                    Ok(()) | Err(trames_vfs::Errno(17)) => (),
                    Err(e) => return Err(e.into()),
                }
            }
            for name in ["proc", "sys"] {
                vfs.mount(
                    Some(format!("/{name}").as_bytes()),
                    format!("/guest/{name}").as_bytes(),
                    None,
                    4096 | 16384,
                    None,
                )?;
            }
            vfs.mount(Some(b"tmpfs"), b"/guest/dev", Some(b"tmpfs"), 2, None)?;
            for (name, major, minor) in [
                ("null", 1, 3),
                ("zero", 1, 5),
                ("full", 1, 7),
                ("random", 1, 8),
                ("urandom", 1, 9),
                ("tty", 5, 0),
            ] {
                vfs.mknod_at(
                    trames_vfs::AT_FDCWD,
                    format!("/guest/dev/{name}").as_bytes(),
                    0o20666,
                    major * 256 + minor,
                )?;
            }
            vfs.mkdir_at(trames_vfs::AT_FDCWD, b"/guest/dev/pts", 0o755)?;
            vfs.mount(Some(b"/dev/pts"), b"/guest/dev/pts", None, 4096, None)?;
            vfs.symlink_at(b"pts/ptmx", trames_vfs::AT_FDCWD, b"/guest/dev/ptmx")?;
            vfs.symlink_at(b"/proc/self/fd", trames_vfs::AT_FDCWD, b"/guest/dev/fd")?;
            vfs.chroot(b"/guest")?;
            vfs.chdir(b"/")?;
        }
        let file = vfs.executable_file_at(trames_vfs::AT_FDCWD, program.as_bytes(), 0)?;
        (file.bytes()?, program)
    } else {
        let bytes = std::fs::read(&program)?;
        vfs.import_executable(b"/program", bytes.clone())?;
        (bytes, "/program".into())
    };
    if let Some(cwd) = command.request.cwd {
        vfs.chdir(cwd.as_bytes())?;
    }
    let argv: Vec<_> = std::iter::once(guest_name)
        .chain(command.request.argv.into_iter().skip(1))
        .collect();
    let mut envp = vec![
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        "LANG=C".into(),
    ];
    for entry in command.request.environment {
        let key = entry.split_once('=').unwrap().0;
        envp.retain(|old: &String| old.split_once('=').is_none_or(|(k, _)| k != key));
        envp.push(entry);
    }
    if command.request.tty && !envp.iter().any(|s| s.starts_with("TERM=")) {
        envp.push("TERM=xterm-256color".into());
    }
    let mut runtime = trames_session::Runtime::<trames_kvm::kernel::Cpu>::load_with_options(
        &bytes,
        &argv,
        &envp,
        vfs,
        host.runtime,
    )?;
    runtime.cpu_count = command.cpus;
    runtime.linux.trace = host.trace;
    if command.request.tty {
        let mut terminal = trames_session::Terminal::default();
        if let Some((rows, columns)) = window_size() {
            terminal.window[..2].copy_from_slice(&rows.to_le_bytes());
            terminal.window[2..4].copy_from_slice(&columns.to_le_bytes());
        }
        runtime.enable_terminal(terminal)?;
    }
    let mut output = Output {
        source: Arc::new(trames_vfs::NativeConsole::stdio()),
        deadline: command.timeout.map(|t| Instant::now() + t),
        signals: Signals::install()?,
        pty: command.request.tty,
        window: None,
    };
    let _raw = RawTerminal::enter(command.request.tty)?;
    let result = runtime.run(&mut output, u64::MAX);
    if let Some(store) = store_sync {
        store.sync()?;
    }
    if let Some(signal) = output.signals.take_signal() {
        return Ok((128 + signal) as u8);
    }
    let exit = result?;
    Ok(output
        .signals
        .take_signal()
        .map_or(exit.code, |signal| (128 + signal) as u8))
}

#[cfg(test)]
mod tests {
    #[test]
    fn store_arguments_require_a_single_value_and_preserve_literal_argv() {
        assert!(super::host_options(vec!["--store".into()]).is_err());
        assert!(
            super::host_options(["--store", "a", "--store", "b"].map(String::from).into()).is_err()
        );
        let (argv, host) = super::host_options(
            ["--store", "a b", "--", "app", "--store"]
                .map(String::from)
                .into(),
        )
        .unwrap();
        assert_eq!(host.store, Some("a b".into()));
        assert_eq!(argv, ["--", "app", "--store"]);
    }
    #[test]
    fn audio_flags_preserve_literal_argv_and_option_values() {
        let (args, host) = super::host_options(
            [
                "--audio",
                "--cwd",
                "--audio-input",
                "--",
                "app",
                "--audio-input",
            ]
            .map(String::from)
            .into(),
        )
        .unwrap();
        assert!(host.store.is_none());
        assert!(host.runtime.audio_output);
        assert!(!host.runtime.audio_input);
        assert_eq!(
            args,
            ["--cwd", "--audio-input", "--", "app", "--audio-input"]
        );
    }
}
