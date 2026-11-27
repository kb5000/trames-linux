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
struct Output<'a> {
    managed: Option<&'a mut crate::managed_io::Service>,
    exec: Option<trames_host::exec::Server>,
    console: Option<trames_host::console::Console>,
    terminals: Option<trames_distro::terminals::NativeTerminals>,
    init: Option<trames_distro::systemd::Monitor>,
    root_signal: Option<u32>,
    shutdown_deadline: Option<Instant>,
    source: Arc<dyn trames_vfs::ConsoleIo>,
    deadline: Option<Instant>,
    signals: Signals,
    pty: bool,
    window: Option<(u16, u16)>,
    display: Option<crate::display::HostDisplay>,
    desktop: Option<crate::desktop::Desktop>,
}
impl GuestOutput for Output<'_> {
    fn bind_host_wake(&mut self, wake: Arc<trames_linux_abi::HostWake>) {
        if let Some(exec) = &mut self.exec {
            exec.bind_wake(wake.clone());
        }
        if let Some(console) = &mut self.console {
            console.bind_wake(wake.clone());
        }
        if let Some(service) = &mut self.managed {
            service.bind_wake(wake);
            service.ready();
        }
    }
    fn host_memory_source(
        &self,
    ) -> Option<Arc<dyn Fn() -> trames_linux_abi::HostMemoryInfo + Send + Sync>> {
        self.managed.as_ref().map(|_| Arc::new(host_memory) as _)
    }
    fn host_spawn(&mut self) -> Option<trames_linux_abi::HostSpawn> {
        self.exec.as_mut()?.spawn()
    }
    fn root_signal(&mut self) -> Option<u32> {
        self.root_signal.take()
    }
    fn source_console(&mut self) -> Arc<dyn trames_vfs::ConsoleIo> {
        self.source.clone()
    }
    fn cancellation_requested(&self) -> bool {
        (self.init.is_none()
            && (self.signals.stopping() || self.managed.as_ref().is_some_and(|s| s.stopping)))
            || self.shutdown_deadline.is_some_and(|d| Instant::now() >= d)
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
        if let Some(service) = &self.managed {
            service.interest(wait)?;
        }
        if let Some(console) = &self.console {
            console.interest(wait)?;
        }
        if let Some(exec) = &self.exec {
            exec.interest(wait)?;
        }
        if let Some(terminals) = &self.terminals {
            wait.retain(terminals.subscribe(wait.signal()));
        }
        if let Some(init) = &self.init {
            wait.retain([init.subscribe(wait.signal())]);
        }
        if let Some(at) = self.shutdown_deadline {
            wait.until(at);
        }
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
    fn poll_host(&mut self, vfs: &trames_vfs::Vfs) -> io::Result<trames_linux_abi::HostState> {
        self.signals.drain();
        if let Some(service) = &mut self.managed {
            service.poll()?;
        }
        let stopping = self.signals.stopping() || self.managed.as_ref().is_some_and(|s| s.stopping);
        if let Some(init) = &mut self.init {
            self.root_signal = init
                .control(stopping)
                .map_err(|e| io::Error::other(e.to_string()))?
                .or(self.root_signal);
            if stopping && self.shutdown_deadline.is_none() {
                self.shutdown_deadline = Some(Instant::now() + std::time::Duration::from_secs(30));
            }
        }
        if let Some(exec) = &mut self.exec {
            exec.poll()?;
            exec.control(vfs);
        }
        if let (Some(console), Some(terminals)) = (&mut self.console, &mut self.terminals) {
            console.poll()?;
            terminals.pump(&mut console.channels);
            console.flush();
        }
        if let Some(display) = &mut self.display {
            display.poll()?;
        }
        if let Some(desktop) = &mut self.desktop {
            if let Err(error) = desktop.poll() {
                // The shared lifecycle boundary reports a host-poll category;
                // retain the native diagnostic on the existing stderr channel.
                eprintln!("trames desktop: {error}");
                return Err(error);
            }
        }
        Ok(trames_linux_abi::HostState::from_cancelled(
            self.cancellation_requested(),
        ))
    }
    fn stdin_ready(&mut self) -> io::Result<bool> {
        if self.console.is_some() {
            return Ok(false);
        }
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
        if !self.pty || self.console.is_some() {
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
        if let Some(console) = &mut self.console {
            return console.write(bytes);
        }
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
    kernel: trames_distro::environment::Kernel,
    mounts: Vec<(String, String, bool)>,
    store: Option<std::path::PathBuf>,
    network: Option<trames_vfs::NetworkMode>,
    network_native: bool,
    network_link: Option<std::path::PathBuf>,
    network_id: Option<u8>,
    network_nat: Option<bool>,
    publish: Vec<trames_distro::environment::Publish>,
    trace: bool,
    display: bool,
    desktop: bool,
    desktop_wayland: bool,
    desktop_renderer: crate::desktop::RendererMode,
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
            "--memory-mib" => {
                let value = args.next().ok_or("missing --memory-mib value")?;
                host.runtime.memory_limits = match trames_distro::environment::memory_mib(&value)
                    .map_err(|e| e.to_string())?
                {
                    None => trames_session::MemoryLimits::host_managed(),
                    Some(mib) => {
                        let bytes = usize::try_from(mib)
                            .ok()
                            .and_then(|v| v.checked_mul(1 << 20))
                            .ok_or("memory size overflow")?;
                        trames_session::MemoryLimits {
                            per_process: bytes,
                            total: bytes,
                        }
                    }
                };
                host.runtime
                    .memory_limits
                    .validate()
                    .map_err(|e| e.to_string())?;
            }
            "--sysctl" => host
                .kernel
                .argument(&args.next().ok_or("missing --sysctl value")?)
                .map_err(|e| e.to_string())?,
            "--mount" | "--mount-ro" => {
                let value = args.next().ok_or("missing GUEST=HOST mount")?;
                let (guest, path) = value
                    .split_once('=')
                    .filter(|(g, h)| g.starts_with('/') && !h.is_empty())
                    .ok_or("mount requires absolute GUEST=HOST")?;
                host.mounts
                    .push((guest.into(), path.into(), arg == "--mount-ro"));
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
                    Some("host") => {
                        host.network_native = true;
                        trames_vfs::NetworkMode::Full
                    }
                    _ => return Err("--network requires off|loopback|outbound|full|host".into()),
                });
            }
            "--network-link" => {
                if host.network_link.is_some() {
                    return Err("duplicate --network-link".into());
                }
                host.network_link = Some(
                    args.next()
                        .filter(|s| s != "--")
                        .ok_or("missing network link")?
                        .into(),
                );
            }
            "--network-id" => {
                if host.network_id.is_some() {
                    return Err("duplicate --network-id".into());
                }
                host.network_id = Some(
                    args.next()
                        .ok_or("missing network id")?
                        .parse()
                        .map_err(|_| "invalid network id")?,
                );
            }
            "--network-nat" => {
                if host.network_nat.is_some() {
                    return Err("duplicate --network-nat".into());
                }
                host.network_nat = Some(
                    args.next()
                        .ok_or("missing network NAT choice")?
                        .parse()
                        .map_err(|_| "network NAT requires true or false")?,
                );
            }
            "--publish" => {
                let value = args.next().ok_or("missing tcp|udp,HOST:PORT,GUEST_PORT")?;
                let fields = value.split(',').collect::<Vec<_>>();
                if fields.len() != 3 || !["tcp", "udp"].contains(&fields[0]) {
                    return Err("publish requires tcp|udp,HOST:PORT,GUEST_PORT".into());
                }
                let guest = fields[2].parse().map_err(|_| "invalid guest port")?;
                if guest == 0 {
                    return Err("guest port must be nonzero".into());
                }
                host.publish.push(trames_distro::environment::Publish {
                    protocol: fields[0].into(),
                    host: fields[1]
                        .parse()
                        .map_err(|_| "invalid published host address")?,
                    guest,
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
            "--desktop-renderer" => {
                host.desktop_renderer = match args.next().as_deref() {
                    Some("auto") => crate::desktop::RendererMode::Auto,
                    Some("software") => crate::desktop::RendererMode::Software,
                    Some("vulkan") => crate::desktop::RendererMode::Vulkan,
                    _ => return Err("--desktop-renderer requires auto|software|vulkan".into()),
                };
            }
            "--desktop" => {
                if host.desktop {
                    return Err("duplicate --desktop".into());
                }
                host.desktop_wayland = match args.next().as_deref() {
                    Some("x11") => false,
                    Some("wayland") => true,
                    _ => return Err("--desktop requires x11|wayland".into()),
                };
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
            _ => {
                let program = !arg.starts_with('-');
                out.push(arg);
                if program {
                    out.extend(args);
                    break;
                }
            }
        }
    }
    if host.desktop && host.display {
        return Err("choose --desktop or --display x11".into());
    }
    if host.desktop && host.display_size.is_some() {
        return Err("desktop windows use guest application sizes".into());
    }
    if host.xwayland_launcher.is_some() && !host.desktop {
        return Err("--xwayland-launcher requires --desktop x11|wayland".into());
    }
    if host.display_size.is_some() && !host.display {
        return Err("--display-size requires --display x11".into());
    }
    if host.network_link.is_some() && host.network_native {
        return Err("host mode cannot use a private network link".into());
    }
    if host.network_link.is_none()
        && (host.network_id.is_some() || host.network_nat.is_some() || !host.publish.is_empty())
    {
        return Err("network id, NAT and publication require --network-link".into());
    }
    if host.network_id.is_some_and(|id| !(2..=254).contains(&id)) {
        return Err("network id must be 2..254".into());
    }
    Ok((out, host))
}
pub fn run(args: Vec<String>, shell: bool) -> Result<u8, Box<dyn std::error::Error>> {
    run_inner(args, shell, None)
}
struct Named<'a> {
    directory: &'a std::path::Path,
    config: &'a trames_distro::environment::Manifest,
    service: &'a mut crate::managed_io::Service,
    clipboard: bool,
}
pub fn run_managed(
    args: Vec<String>,
    directory: &std::path::Path,
    config: &trames_distro::environment::Manifest,
    service: &mut crate::managed_io::Service,
    clipboard: bool,
) -> Result<u8, Box<dyn std::error::Error>> {
    run_inner(
        args,
        false,
        Some(Named {
            directory,
            config,
            service,
            clipboard,
        }),
    )
}
fn run_inner(
    args: Vec<String>,
    shell: bool,
    mut named: Option<Named<'_>>,
) -> Result<u8, Box<dyn std::error::Error>> {
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
    let mut vfs = if let Some(named) = &named {
        trames_vfs::Vfs::open_disk(named.directory.join("store"), Default::default())?
    } else if let Some(path) = &store {
        crate::store::open(path, command.rootfs.as_deref().map(std::path::Path::new))?
    } else {
        trames_vfs::Vfs::default()
    };
    if let Some(directory) = &host.network_link {
        let id = host.network_id.unwrap_or(2);
        let fabric = trames_net::Fabric::new()?;
        fabric.attach(directory, id, host.network_nat.unwrap_or(true))?;
        for p in &host.publish {
            let address = if p.host.is_ipv4() {
                std::net::Ipv4Addr::new(10, 0, 0, id).into()
            } else {
                std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, u16::from(id)).into()
            };
            fabric.publish(
                p.host,
                std::net::SocketAddr::new(address, p.guest),
                if p.protocol == "tcp" { 1 } else { 2 },
            )?;
        }
        vfs.configure_virtual_network(
            &fabric,
            id,
            host.network.unwrap_or(trames_vfs::NetworkMode::Outbound),
        )?;
    } else if let Some(network) = host.network {
        if host.network_native {
            vfs.configure_network(network)?;
        } else {
            vfs.configure_local_fabric(network)?;
        }
    }

    host.kernel.apply(&vfs)?;
    // Seed pseudo inodes before attaching a rootfs, as in shared init boot.
    vfs.set_runtime_info(1, Default::default());
    let store_sync = (store.is_some() || named.is_some()).then(|| vfs.host_control_view());
    if named.is_some() {
        trames_distro::boot::prepare_runtime_mounts(&mut vfs)?;
        trames_distro::systemd::ensure_machine_identity(&mut vfs)?;
    }
    for (guest, path, readonly) in &host.mounts {
        let mut part = String::new();
        for component in guest.split('/').filter(|v| !v.is_empty()) {
            if component == ".." || component == "." {
                return Err("mount target must not contain dot segments".into());
            }
            part.push('/');
            part.push_str(component);
            match vfs.mkdir_at(trames_vfs::AT_FDCWD, part.as_bytes(), 0o755) {
                Ok(()) | Err(trames_vfs::Errno(17)) => (),
                Err(e) => return Err(e.into()),
            }
        }
        vfs.mount_host(guest.as_bytes(), path, *readonly)?;
    }
    let init = if named
        .as_ref()
        .is_some_and(|n| n.config.boot.mode == trames_distro::environment::Init::Systemd)
    {
        Some(trames_distro::systemd::prepare(&mut vfs)?)
    } else {
        None
    };
    let (bytes, guest_name) = if store.is_some() || command.rootfs.is_some() || named.is_some() {
        if store.is_none() && named.is_none() {
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
    if named.is_some() {
        envp.extend([
            "HOME=/root".into(),
            "LANG=C.UTF-8".into(),
            "TERM=xterm-256color".into(),
        ]);
    }
    if init.is_some() {
        envp.extend([
            "container=trames".into(),
            "NOTIFY_SOCKET=/run/host/notify".into(),
        ]);
    }
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
        // GUI terminals allocate their own PTYs. The compatibility /dev/pts
        // bind has no pts/ptmx entry; give this desktop a real devpts instance.
        vfs.mount(
            Some(b"devpts"),
            b"/dev/pts",
            Some(b"devpts"),
            0,
            Some(b"newinstance,mode=0620,ptmxmode=0666"),
        )?;

        let server = vfs.enable_wayland(b"/run/user/0")?;
        envp.extend([
            "HOME=/root".into(),
            "XDG_RUNTIME_DIR=/run/user/0".into(),
            "WAYLAND_DISPLAY=wayland-0".into(),
            "GDK_BACKEND=wayland".into(),
            "XMODIFIERS=@im=trames".into(),
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
    if init.is_some() && desktop_server.is_some() {
        trames_distro::systemd::file(&mut vfs,b"/run/systemd/system/console-getty.service.d/60-wayland.conf",b"[Service]\nEnvironment=XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY=wayland-0 GDK_BACKEND=wayland\n")?;
    }
    let terminals = if let Some(n) = &named {
        let launcher = trames_distro::terminals::launcher_at(
            &n.config.session.shell,
            Some(&n.config.session.cwd),
        );
        Some(
            if init.is_some() {
                trames_distro::terminals::NativeTerminals::for_systemd_launcher(&mut vfs, &launcher)
            } else {
                trames_distro::terminals::NativeTerminals::new(&mut vfs)
            }
            .map_err(io::Error::other)?,
        )
    } else {
        None
    };
    let console_path = named
        .as_ref()
        .map(|n| crate::environments::console_path(n.directory))
        .transpose()?;
    let console = console_path
        .as_ref()
        .map(|p| trames_host::console::Console::bind(p))
        .transpose()?;
    let exec = console_path
        .as_ref()
        .map(|p| trames_host::exec::Server::bind(p, envp.clone()))
        .transpose()?;
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
        .map(|s| {
            if named.as_ref().is_some_and(|n| !n.clipboard) {
                crate::desktop::Desktop::open_with_policy(
                    s,
                    host.desktop_renderer,
                    host.desktop_wayland,
                    false,
                )
            } else if host.desktop_wayland {
                crate::desktop::Desktop::open_wayland(s, host.desktop_renderer)
            } else {
                crate::desktop::Desktop::open_mode(s, host.desktop_renderer)
            }
        })
        .transpose()?;
    let source = console
        .as_ref()
        .map(|c| c.source())
        .unwrap_or_else(|| Arc::new(trames_vfs::NativeConsole::stdio()));
    let mut output = Output {
        managed: named.as_mut().map(|n| &mut *n.service),
        exec,
        console,
        terminals,
        init,
        root_signal: None,
        shutdown_deadline: None,
        source,
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
    if output
        .shutdown_deadline
        .is_some_and(|d| Instant::now() >= d)
    {
        return Err(
            "systemd did not stop within 30 seconds; runtime was cancelled and synced".into(),
        );
    }
    if output.managed.as_ref().is_some_and(|s| s.stopping)
        && matches!(&result, Err(trames_session::RuntimeError::Cancelled))
    {
        return Ok(0);
    }
    let exit = result?;
    Ok(output
        .signals
        .take_signal()
        .map_or(exit.code, |signal| (128 + signal) as u8))
}

fn host_memory() -> trames_linux_abi::HostMemoryInfo {
    let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let bytes = |name: &str| {
        text.lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .split_whitespace()
                    .next()?
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0)
            .saturating_mul(1024)
    };
    trames_linux_abi::HostMemoryInfo {
        total: bytes("MemTotal:"),
        free: bytes("MemFree:"),
        available: bytes("MemAvailable:"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn network_options_preserve_literal_argv_and_reject_incomplete_links() {
        let parse = |a: &[&str]| super::host_options(a.iter().map(|s| s.to_string()).collect());
        let (args, host) = parse(&[
            "--network-link",
            "/tmp/lan",
            "--network-id",
            "9",
            "--network-nat",
            "false",
            "--publish",
            "udp,[::1]:38000,53",
            "/bin/echo",
            "--publish",
            "literal",
        ])
        .unwrap();
        assert_eq!(args, ["/bin/echo", "--publish", "literal"]);
        assert_eq!(host.network_id, Some(9));
        assert_eq!(host.network_nat, Some(false));
        assert_eq!(host.publish[0].guest, 53);
        for a in [
            vec!["--network-id", "2"],
            vec!["--network-link", "/tmp/lan", "--network-id", "1"],
            vec![
                "--network-link",
                "/tmp/lan",
                "--publish",
                "icmp,127.0.0.1:1,1",
            ],
            vec![
                "--network-link",
                "/tmp/lan",
                "--publish",
                "tcp,127.0.0.1:1,0",
            ],
        ] {
            assert!(parse(&a).is_err());
        }
    }
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
