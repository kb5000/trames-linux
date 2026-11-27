// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-environment control on the original foreground wait. No PID-file signalling.
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
};
pub struct Service {
    wake: Option<std::sync::Arc<trames_linux_abi::HostWake>>,
    listener: Option<UnixListener>,
    path: PathBuf,
    identity: (u64, u64),
    ready: Option<UnixStream>,
    ready_sent: bool,
    notifier: Option<crate::user_service::Notifier>,
    pub ready_error: Option<io::Error>,
    peers: Vec<UnixStream>,
    pub stopping: bool,
}
pub fn startup_error(ready: &mut Option<UnixStream>, error: &str) {
    if let Some(mut socket) = ready.take() {
        let b = error.as_bytes();
        let b = &b[..b.len().min(8192)];
        let _ = socket.write_all(b"E");
        let _ = socket.write_all(&(b.len() as u32).to_be_bytes());
        let _ = socket.write_all(b);
    }
}
impl Service {
    pub fn open(
        path: &Path,
        ready: Option<UnixStream>,
        notifier: Option<crate::user_service::Notifier>,
    ) -> io::Result<Self> {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_socket() => fs::remove_file(path)?,
            Ok(_) => return Err(io::Error::other("control path is not a socket")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let m = fs::symlink_metadata(path)?;
        Ok(Self {
            wake: None,
            listener: Some(listener),
            path: path.into(),
            identity: (m.dev(), m.ino()),
            ready,
            ready_sent: false,
            notifier,
            ready_error: None,
            peers: vec![],
            stopping: false,
        })
    }
    pub fn bind_wake(&mut self, wake: std::sync::Arc<trames_linux_abi::HostWake>) {
        self.wake = Some(wake);
    }
    pub fn ready(&mut self) {
        if self.ready_sent {
            return;
        }
        self.ready_sent = true;
        if let Some(notifier) = self.notifier.take() {
            if let Err(error) = notifier.ready() {
                self.ready_error = Some(error);
                self.stopping = true;
            }
        }
        if let Some(mut ready) = self.ready.take() {
            if ready.write_all(b"R").is_err() {
                self.stopping = true;
            }
        }
    }
    pub fn interest(&self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
        if let Some(l) = &self.listener {
            wait.watch(l, false)?;
        }
        for p in &self.peers {
            wait.watch(p, false)?;
        }
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<()> {
        if let Some(l) = &self.listener {
            match l.accept() {
                Ok((p, _)) if self.peers.len() < 16 => {
                    p.set_nonblocking(true)?;
                    self.peers.push(p);
                    if let Some(wake) = &self.wake {
                        wake.notify();
                    }
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
        }
        let mut retained = vec![];
        for mut p in self.peers.drain(..) {
            let mut b = [0];
            match p.read(&mut b) {
                Ok(1) if b[0] == b'S' => {
                    self.stopping = true;
                    retained.push(p)
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => retained.push(p),
                _ => (),
            }
        }
        self.peers = retained;
        Ok(())
    }
    /// Retire pathname ownership before releasing the environment owner lock.
    pub fn retire(&mut self) {
        self.listener.take();
        if fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
    pub fn finish(&mut self, result: &Result<u8, Box<dyn std::error::Error>>) {
        if !self.ready_sent {
            startup_error(
                &mut self.ready,
                &result
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "guest exited during startup".into()),
            );
        }
        for p in &mut self.peers {
            let _ = p.set_nonblocking(false);
            let _ = p.set_write_timeout(Some(std::time::Duration::from_secs(1)));
            let _ = p.write_all(if result.is_ok() { b"D" } else { b"E" });
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn late_control_peer_wakes_existing_wait_and_receives_ack_after_retirement() {
        let path = PathBuf::from("/tmp").join(format!("trames-control-{}", std::process::id()));
        let (mut reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut service = Service::open(&path, Some(writer), None).unwrap();
        let mut wait = trames_linux_abi::HostWait::new().unwrap();
        service.bind_wake(wait.wake());
        service.ready();
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"R");
        wait.begin();
        service.interest(&mut wait).unwrap();
        let mut peer = UnixStream::connect(&path).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        service.poll().unwrap(); // Accepted but not yet readable: the old wait lacks this FD.
        let began = Instant::now();
        wait.until(began + Duration::from_secs(2));
        wait.wait().unwrap();
        assert!(began.elapsed() < Duration::from_secs(1));
        wait.begin();
        service.interest(&mut wait).unwrap();
        peer.write_all(b"S").unwrap();
        wait.until(Instant::now() + Duration::from_secs(2));
        wait.wait().unwrap();
        service.poll().unwrap();
        assert!(service.stopping);
        service.retire();
        assert!(!path.exists());
        service.finish(&Ok(0));
        peer.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"D");
    }
    #[test]
    fn cleanup_preserves_a_replacement_path_and_reports_startup_failure() {
        let path =
            PathBuf::from("/tmp").join(format!("trames-control-replaced-{}", std::process::id()));
        let (mut reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut service = Service::open(&path, Some(writer), None).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"replacement").unwrap();
        service.retire();
        service.finish(&Err("startup failed".into()));
        let mut header = [0; 5];
        reader.read_exact(&mut header).unwrap();
        assert_eq!(header[0], b'E');
        let mut message = vec![0; u32::from_be_bytes(header[1..].try_into().unwrap()) as usize];
        reader.read_exact(&mut message).unwrap();
        assert_eq!(message, b"startup failed");
        drop(service);
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        fs::remove_file(path).unwrap();
    }
}
