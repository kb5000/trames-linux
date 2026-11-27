// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-user systemd owns the existing foreground environment worker directly.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::{fs, io, path::PathBuf};
use trames_distro::{
    environment::Result,
    registry::{self, Registry},
};

fn location(r: &Registry, n: &str) -> Result<(String, PathBuf)> {
    let i = registry::instance(&r.require(n)?)?;
    let socket = r.socket(&i.uuid)?;
    let key = socket
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or("invalid socket identity")?;
    let unit = format!("trames-{}-{}.service", i.uuid, &key[..12]);
    let config = if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(path)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(".config")
    };
    if !config.is_absolute() {
        return Err("user configuration directory must be absolute".into());
    }
    Ok((unit.clone(), config.join("systemd/user").join(unit)))
}
fn marker(unit: &str) -> String {
    format!("# Trames managed user service v1: {unit}\n")
}
fn owned(unit: &str, path: &std::path::Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && fs::read_to_string(path)?.starts_with(&marker(unit)) => Ok(true),
        Ok(_) => Err("refusing to replace an unmanaged user unit".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
// systemd parses argv itself. There is no shell; escape its environment/specifier syntax.
fn quoted(s: &str, exec: bool) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' if exec => out.push_str("$$"),
            c if c.is_control() => {
                for b in c.to_string().bytes() {
                    out.push_str(&format!("\\x{b:02x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('\"');
    out
}
fn render(unit: &str, exe: &str, root: &str, uuid: &str) -> String {
    let command = |op| [exe, "env", op, uuid].map(|v| quoted(v, true)).join(" ");
    format!("{}[Unit]\nDescription=Trames environment {uuid}\nStartLimitIntervalSec=60\nStartLimitBurst=3\n\n[Service]\nType=notify\nNotifyAccess=main\nEnvironment={}\nExecStart={}\nExecStop={}\nRestart=on-failure\nRestartSec=2\nSuccessExitStatus=130 143\nTimeoutStartSec=90\nTimeoutStopSec=45\nKillMode=mixed\nUMask=0077\n\n[Install]\nWantedBy=default.target\n",
        marker(unit), quoted(&format!("TRAMES_HOME={root}"), false), command("_service"), command("_stop"))
}
pub fn installed(r: &Registry, n: &str) -> Result<bool> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (r, n);
        Ok(false)
    }
    #[cfg(target_os = "linux")]
    {
        let (unit, path) = location(r, n)?;
        owned(&unit, &path)
    }
}
pub fn inspect(r: &Registry, n: &str) -> Result<serde_json::Value> {
    let (unit, path) = location(r, n)?;
    let installed = owned(&unit, &path)?;
    let enabled = path
        .parent()
        .unwrap()
        .join("default.target.wants")
        .join(&unit)
        .is_symlink();
    Ok(serde_json::json!({"unit":unit,"path":path,"installed":installed,"enabled":enabled}))
}
fn systemctl(args: &[&str]) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err("systemd user services require a Linux host".into())
    }
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl --user {} failed ({status}); inspect the user journal",
                args.join(" ")
            )
            .into())
        }
    }
}
pub fn control(r: &Registry, n: &str, op: &str) -> Result<()> {
    let (unit, path) = location(r, n)?;
    if !owned(&unit, &path)? {
        return Err("environment user service is not installed".into());
    }
    if matches!(op, "start" | "restart") {
        systemctl(&["reset-failed", &unit])?;
    }
    systemctl(&[op, &unit])
}
pub fn logs(r: &Registry, n: &str) -> Result<()> {
    let (unit, _) = location(r, n)?;
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("journalctl")
            .args(["--user", "--unit", &unit, "--no-pager", "--lines=200"])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err("journalctl failed".into())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = unit;
        Err("systemd user services require a Linux host".into())
    }
}
pub fn dispatch(r: &Registry, op: &str, args: &[String]) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err("systemd user services require a Linux host".into());
    }
    if args.is_empty() || args.len() > 2 || args.get(1).is_some_and(|a| a != "--now") {
        return Err("usage: env enable|disable NAME [--now]".into());
    }
    let n = &args[0];
    let (unit, path) = location(r, n)?;
    // Serialize this unit's configuration across systemctl calls. The lifecycle
    // lock is released before ExecStop, which acquires it in the existing worker protocol.
    use std::os::unix::fs::OpenOptionsExt;
    let config_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(r.root.join("locks").join(format!(".user-service-{unit}")))?;
    config_lock.lock()?;
    let lifecycle = r.lifecycle_lock(n)?;
    let exists = owned(&unit, &path)?;
    let now = args.len() == 2;
    if op == "enable" {
        if now && !exists && r.running(n)? {
            return Err(
                "stop the existing environment before enabling its user service with --now".into(),
            );
        }
        registry::private_dir(path.parent().unwrap())?;
        let i = registry::instance(&r.require(n)?)?;
        let exe = std::env::current_exe()?;
        let content = render(
            &unit,
            exe.to_str().ok_or("executable path must be UTF-8")?,
            r.root.to_str().ok_or("registry path must be UTF-8")?,
            &i.uuid,
        );
        registry::atomic_write(&path, content.as_bytes())?;
        drop(lifecycle);
        systemctl(&["daemon-reload"])?;
        if now {
            systemctl(&["enable", "--now", &unit])?;
        } else {
            systemctl(&["enable", &unit])?;
        }
    } else {
        drop(lifecycle);
        if exists {
            if now {
                systemctl(&["disable", "--now", &unit])?;
            } else {
                systemctl(&["disable", &unit])?;
            }
            fs::remove_file(&path)?;
            systemctl(&["daemon-reload"])?;
        }
        // --now also stops a pre-existing ordinary worker, or one kept alive
        // by an earlier disable without --now.
        if now {
            crate::environments::stop_direct(r, n)?;
        }
    }
    println!(
        "{} {unit}",
        if op == "enable" {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    Ok(())
}

pub struct Notifier(std::os::unix::net::UnixDatagram);
impl Notifier {
    pub fn open() -> Result<Self> {
        let name = std::env::var_os("NOTIFY_SOCKET")
            .ok_or("the environment service must be started by the systemd user manager")?;
        #[cfg(target_os = "linux")]
        let socket = std::os::unix::net::UnixDatagram::unbound()?;
        #[cfg(target_os = "linux")]
        socket.set_write_timeout(Some(std::time::Duration::from_secs(1)))?;
        #[cfg(target_os = "linux")]
        {
            use std::os::{linux::net::SocketAddrExt, unix::ffi::OsStrExt};
            let bytes = name.as_bytes();
            if let Some(abstract_name) = bytes.strip_prefix(b"@") {
                socket.connect_addr(&std::os::unix::net::SocketAddr::from_abstract_name(
                    abstract_name,
                )?)?;
            } else {
                socket.connect(&name)?;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err("systemd user services require a Linux host".into())
        }
        #[cfg(target_os = "linux")]
        Ok(Self(socket))
    }
    pub fn ready(&self) -> io::Result<()> {
        self.0
            .send(b"READY=1\nSTATUS=Environment command sockets and execution workers are ready")
            .map(|_| ())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_uses_literal_argv_stable_uuid_and_foreground_notifications() {
        let unit = render(
            "trames-test.service",
            "/a path/$exe%/trames",
            "/home/a \"$data%\"",
            "uuid",
        );
        assert!(unit.contains("Type=notify\nNotifyAccess=main"));
        assert!(unit.contains("ExecStart=\"/a path/$$exe%%/trames\" \"env\" \"_service\" \"uuid\""));
        assert!(unit.contains("Environment=\"TRAMES_HOME=/home/a \\\"$data%%\\\"\""));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!quoted("one\ntwo\rthree", true).contains('\n'));
    }
}
