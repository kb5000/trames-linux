// SPDX-License-Identifier: GPL-3.0-or-later
use std::{
    io::{self, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use trames_linux_abi::{GuestOutput, OutputFd};
struct Output {
    source: Arc<dyn trames_vfs::ConsoleIo>,
    deadline: Option<Instant>,
}
impl GuestOutput for Output {
    fn source_console(&mut self) -> Arc<dyn trames_vfs::ConsoleIo> {
        self.source.clone()
    }
    fn cancellation_requested(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }
    fn waits_for_host(&self) -> bool {
        self.deadline.is_some()
    }
    fn host_interest(&mut self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
        if let Some(deadline) = self.deadline {
            wait.until(deadline);
        }
        Ok(())
    }
    fn write(&mut self, fd: OutputFd, bytes: &[u8]) -> io::Result<usize> {
        match fd {
            OutputFd::Stdout => io::stdout().write(bytes),
            OutputFd::Stderr => io::stderr().write(bytes),
        }
    }
}
pub fn run(args: Vec<String>) -> Result<u8, Box<dyn std::error::Error>> {
    let mut args = args.into_iter();
    let mut cpus = 1;
    let mut timeout = None;
    let mut root = None;
    loop {
        match args.next().as_deref() {
            Some("--") => break,
            Some("--cpus") => {
                cpus = args.next().ok_or("missing CPU count")?.parse::<usize>()?;
                if !(1..=32).contains(&cpus) {
                    return Err("CPU count must be 1..=32".into());
                }
            }
            Some("--timeout-ms") => {
                let ms = args.next().ok_or("missing timeout")?.parse::<u64>()?;
                if ms == 0 || ms > 86_400_000 {
                    return Err("timeout must be 1..=86400000 ms".into());
                }
                timeout = Some(Duration::from_millis(ms));
            }
            Some("--rootfs") => root = Some(args.next().ok_or("missing rootfs directory")?),
            _ => {
                return Err(
                    "usage: trames run [--cpus N] [--timeout-ms N] [--rootfs DIR] -- ELF [ARG...]"
                        .into(),
                )
            }
        }
    }
    let program = args.next().ok_or("missing ELF program after --")?;
    // Never read an ELF or mount a host directory on an unsupported machine.
    trames_kvm::Doctor::inspect().require_probe()?;
    let mut vfs = trames_vfs::Vfs::default();
    let (bytes, guest_name) = if let Some(root) = root {
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
    let argv: Vec<_> = std::iter::once(guest_name).chain(args).collect();
    let envp = vec![
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        "LANG=C".into(),
    ];
    let mut runtime = trames_session::Runtime::<trames_kvm::kernel::Cpu>::load_with_vfs(
        &bytes, &argv, &envp, vfs,
    )?;
    runtime.cpu_count = cpus;
    let mut output = Output {
        source: Arc::new(trames_vfs::NativeConsole::stdio()),
        deadline: timeout.map(|t| Instant::now() + t),
    };
    let exit = runtime.run(&mut output, u64::MAX)?;
    Ok(exit.code)
}
