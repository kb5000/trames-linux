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
pub fn run(args: Vec<String>, shell: bool) -> Result<u8, Box<dyn std::error::Error>> {
    use std::io::IsTerminal;
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
        if command.rootfs.is_none() {
            return Err("shell requires --rootfs DIR".into());
        }
        command.request.tty |= !no_pty && io::stdin().is_terminal();
    }
    let program = command.request.argv.first().unwrap().clone();
    // Probe before reading an ELF or mounting a host directory on unsupported hosts.
    trames_kvm::Doctor::inspect().require_probe()?;
    let mut vfs = trames_vfs::Vfs::default();
    let (bytes, guest_name) = if let Some(root) = command.rootfs {
        if !program.starts_with('/') {
            return Err("with --rootfs the program must be an absolute guest path".into());
        }
        vfs.mount_host(b"/", &root, false)?;
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
    let mut runtime = trames_session::Runtime::<trames_kvm::kernel::Cpu>::load_with_vfs(
        &bytes, &argv, &envp, vfs,
    )?;
    runtime.cpu_count = command.cpus;
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
    if let Some(signal) = output.signals.take_signal() {
        return Ok((128 + signal) as u8);
    }
    let exit = result?;
    Ok(output
        .signals
        .take_signal()
        .map_or(exit.code, |signal| (128 + signal) as u8))
}
