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
    display: Option<crate::display::HostDisplay>,
    desktop: Option<crate::desktop::Desktop>,
}
impl GuestOutput for Output {
    fn source_console(&mut self) -> Arc<dyn trames_vfs::ConsoleIo> {
        self.source.clone()
    }
    fn cancellation_requested(&self) -> bool {
        self.signals.stopping()
            || self.deadline.is_some_and(|d| Instant::now() >= d)
            || self
                .display
                .as_ref()
                .is_some_and(|display| display.closed())
    }
    fn waits_for_host(&self) -> bool {
        true
    }
    fn host_interest(&mut self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
        wait.watch(&self.signals, false)?;
        if let Some(display) = &self.display {
            display.interest(wait)?;
        }
        if let Some(desktop) = &mut self.desktop {
            desktop.interest(wait)?;
        }
        if let Some(deadline) = self.deadline {
            wait.until(deadline);
        }
        Ok(())
    }
    fn poll_host(&mut self, _: &trames_vfs::Vfs) -> io::Result<trames_linux_abi::HostState> {
        self.signals.drain();
        if let Some(display) = &mut self.display {
            display.poll()?;
        }
        if let Some(desktop) = &mut self.desktop {
            desktop.poll()?;
        }
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
    display: bool,
    desktop: bool,
    xwayland_launcher: Option<std::path::PathBuf>,
    display_size: Option<(u32, u32)>,
    vulkan_icd: Option<std::path::PathBuf>,
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
            "--vulkan" => host.runtime.vulkan_accelerator = true,
            "--vulkan-icd" => {
                if host.vulkan_icd.is_some() {
                    return Err("duplicate --vulkan-icd".into());
                }
                host.vulkan_icd = Some(
                    args.next()
                        .filter(|path| path != "--")
                        .ok_or("missing --vulkan-icd library")?
                        .into(),
                );
                host.runtime.vulkan_accelerator = true;
            }
            "--desktop" => {
                if host.desktop {
                    return Err("duplicate --desktop".into());
                }
                if args.next().as_deref() != Some("x11") {
                    return Err("--desktop requires x11".into());
                }
                host.desktop = true;
            }
            "--xwayland-launcher" => {
                if host.xwayland_launcher.is_some() {
                    return Err("duplicate --xwayland-launcher".into());
                }
                host.xwayland_launcher = Some(
                    args.next()
                        .filter(|p| p != "--")
                        .ok_or("missing --xwayland-launcher executable")?
                        .into(),
                );
            }
            "--display" => {
                if host.display {
                    return Err("duplicate --display".into());
                }
                if args.next().as_deref() != Some("x11") {
                    return Err("--display requires x11".into());
                }
                host.display = true;
                host.runtime.vulkan_accelerator = true;
            }
            "--display-size" => {
                if host.display_size.is_some() {
                    return Err("duplicate --display-size".into());
                }
                let value = args.next().ok_or("--display-size requires WIDTHxHEIGHT")?;
                let (width, height) = value
                    .split_once('x')
                    .ok_or("--display-size requires WIDTHxHEIGHT")?;
                let (width, height): (u32, u32) = (
                    width.parse().map_err(|_| "invalid display width")?,
                    height.parse().map_err(|_| "invalid display height")?,
                );
                if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                    return Err("display dimensions must be 1..4096".into());
                }
                host.display_size = Some((width, height));
            }
            "--rootfs" | "--cpus" | "--timeout-ms" | "--setenv" | "--cwd" | "-w" => {
                out.push(arg);
                if let Some(value) = args.next() {
                    out.push(value);
                }
            }
            _ => out.push(arg),
        }
    }
    if host.desktop && host.display {
        return Err("choose --desktop x11 or --display x11".into());
    }
    if host.desktop && host.display_size.is_some() {
        return Err("desktop windows use guest application sizes".into());
    }
    if host.xwayland_launcher.is_some() && !host.desktop {
        return Err("--xwayland-launcher requires --desktop x11".into());
    }
    if host.display_size.is_some() && !host.display {
        return Err("--display-size requires --display x11".into());
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
            // Keep the shared runtime's FUSE device and connection semantics.
            // A recreated character node alone does not retain that provider.
            vfs.mknod_at(trames_vfs::AT_FDCWD, b"/guest/dev/fuse", 0o100666, 0)?;
            vfs.mount(Some(b"/dev/fuse"), b"/guest/dev/fuse", None, 4096, None)?;
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
        if host.desktop {
            "LANG=C.UTF-8".into()
        } else {
            "LANG=C".into()
        },
    ];
    let desktop_server = if host.desktop {
        // Native IPC belongs to this session. A direct host-root grant cannot
        // create guest socket inodes, and must not export them to the host.
        for path in [b"/run".as_slice(), b"/run/user", b"/run/user/0", b"/tmp"] {
            match vfs.mkdir_at(trames_vfs::AT_FDCWD, path, 0o755) {
                Ok(()) | Err(trames_vfs::Errno(17)) => (),
                Err(e) => return Err(e.into()),
            }
        }
        vfs.mount(
            Some(b"tmpfs"),
            b"/run/user/0",
            Some(b"tmpfs"),
            0,
            Some(b"mode=0700"),
        )?;
        vfs.mount(
            Some(b"tmpfs"),
            b"/tmp",
            Some(b"tmpfs"),
            0,
            Some(b"mode=1777"),
        )?;
        vfs.mkdir_at(trames_vfs::AT_FDCWD, b"/tmp/.X11-unix", 0o1777)?;
        let server = vfs.enable_wayland(b"/run/user/0")?;
        envp.extend([
            "HOME=/root".into(),
            "XDG_RUNTIME_DIR=/run/user/0".into(),
            "WAYLAND_DISPLAY=wayland-0".into(),
            "GDK_BACKEND=wayland".into(),
        ]);
        Some(server)
    } else {
        None
    };
    if let Some(path) = host.xwayland_launcher {
        trames_distro::install_xwayland_launcher(&mut std::fs::File::open(path)?, &mut vfs)
            .map_err(|e| format!("install Xwayland launcher: {e}"))?;
    }
    for entry in command.request.environment {
        let key = entry.split_once('=').unwrap().0;
        envp.retain(|old: &String| old.split_once('=').is_none_or(|(k, _)| k != key));
        envp.push(entry);
    }
    if command.request.tty && !envp.iter().any(|s| s.starts_with("TERM=")) {
        envp.push("TERM=xterm-256color".into());
    }
    if let Some(path) = host.vulkan_icd {
        trames_distro::install_vulkan_icd(&mut std::fs::File::open(path)?, &mut vfs)
            .map_err(|error| format!("install guest Vulkan ICD: {error}"))?;
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
    let display = if host.display {
        let (width, height) = host.display_size.unwrap_or((1024, 768));
        Some(crate::display::HostDisplay::open(
            runtime.vulkan_display.clone(),
            width,
            height,
        )?)
    } else {
        None
    };
    if command.request.tty {
        let mut terminal = trames_session::Terminal::default();
        if let Some((rows, columns)) = window_size() {
            terminal.window[..2].copy_from_slice(&rows.to_le_bytes());
            terminal.window[2..4].copy_from_slice(&columns.to_le_bytes());
        }
        runtime.enable_terminal(terminal)?;
    }
    let desktop = desktop_server
        .map(crate::desktop::Desktop::open)
        .transpose()?;
    let mut output = Output {
        source: Arc::new(trames_vfs::NativeConsole::stdio()),
        deadline: command.timeout.map(|t| Instant::now() + t),
        signals: Signals::install()?,
        pty: command.request.tty,
        window: None,
        display,
        desktop,
    };
    let _raw = RawTerminal::enter(command.request.tty)?;
    let result = runtime.run(&mut output, u64::MAX);
    if let Some(store) = store_sync {
        store.sync()?;
    }
    if output
        .display
        .as_ref()
        .is_some_and(|display| display.closed())
    {
        return Ok(0);
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
    fn display_and_gpu_options_preserve_guest_arguments_and_validate_geometry() {
        let (args, host) = super::host_options(
            [
                "--vulkan-icd",
                "lib api.so",
                "--display-size",
                "800x600",
                "--display",
                "x11",
                "--",
                "app",
                "--display",
            ]
            .map(String::from)
            .into(),
        )
        .unwrap();
        assert_eq!(args, ["--", "app", "--display"]);
        assert!(host.runtime.vulkan_accelerator && host.display);
        assert_eq!(host.display_size, Some((800, 600)));
        assert_eq!(host.vulkan_icd, Some("lib api.so".into()));
        for args in [
            vec!["--display"],
            vec!["--display", "invalid"],
            vec!["--display-size", "800x600"],
            vec!["--display", "x11", "--display-size", "0x600"],
            vec!["--display", "x11", "--display-size", "800x4097"],
            vec!["--vulkan-icd", "--"],
        ] {
            assert!(super::host_options(args.into_iter().map(String::from).collect()).is_err());
        }
    }
    #[test]
    fn desktop_options_keep_software_mode_and_guest_arguments() {
        let (args, host) = super::host_options(
            [
                "--desktop",
                "x11",
                "--xwayland-launcher",
                "launcher",
                "--",
                "app",
                "--desktop",
            ]
            .map(String::from)
            .into(),
        )
        .unwrap();
        assert_eq!(args, ["--", "app", "--desktop"]);
        assert!(host.desktop && !host.runtime.vulkan_accelerator);
        assert_eq!(host.xwayland_launcher, Some("launcher".into()));
        for args in [
            vec!["--desktop"],
            vec!["--desktop", "other"],
            vec!["--desktop", "x11", "--desktop", "x11"],
            vec!["--desktop", "x11", "--display", "x11"],
            vec!["--desktop", "x11", "--display-size", "800x600"],
            vec!["--xwayland-launcher", "launcher"],
        ] {
            assert!(super::host_options(args.into_iter().map(String::from).collect()).is_err());
        }
    }
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
