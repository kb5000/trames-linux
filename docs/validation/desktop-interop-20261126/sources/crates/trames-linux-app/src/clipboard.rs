// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard transfers are pumped by the original foreground wait, including
//! pipe backpressure and cancellation. No clipboard service thread is created.
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use trames_shared::{HostWait, HostWake, NativePipe};
pub const LIMIT: usize = 16384;
pub enum Request {
    Read(NativePipe),
    Write(NativePipe),
}
#[derive(Default)]
struct Mailbox {
    requests: VecDeque<Request>,
    wake: Option<Arc<HostWake>>,
}
#[derive(Default)]
pub struct Bridge(Mutex<Mailbox>);
impl Bridge {
    pub fn arm(&self, wait: &HostWait) {
        let mut m = self.0.lock().unwrap();
        m.wake = Some(wait.wake());
        if !m.requests.is_empty() {
            wait.wake().notify();
        }
    }
    fn push(&self, r: Request) -> io::Result<()> {
        let mut m = self.0.lock().unwrap();
        if m.requests.len() >= 16 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        m.requests.push_back(r);
        if let Some(w) = &m.wake {
            w.notify();
        }
        Ok(())
    }
    pub fn take(&self) -> VecDeque<Request> {
        std::mem::take(&mut self.0.lock().unwrap().requests)
    }
}
impl trames_wayland::HostClipboard for Bridge {
    fn read(&self, fd: std::os::fd::OwnedFd) -> io::Result<()> {
        self.push(Request::Read(NativePipe::import(fd)?))
    }
    fn write(&self, pipe: NativePipe) -> io::Result<()> {
        self.push(Request::Write(pipe))
    }
}
pub struct Reader {
    pub pipe: NativePipe,
    pub bytes: Vec<u8>,
    pub deadline: Instant,
}
impl Reader {
    pub fn new(pipe: NativePipe) -> Self {
        Self {
            pipe,
            bytes: vec![],
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
    pub fn interest(&self, wait: &mut HostWait) -> io::Result<()> {
        wait.watch(&self.pipe, false)?;
        wait.until(self.deadline);
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<Option<String>> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut b = [0; 4096];
        loop {
            match self.pipe.read(&mut b) {
                Ok(0) => {
                    return String::from_utf8(std::mem::take(&mut self.bytes))
                        .map(Some)
                        .map_err(|_| io::ErrorKind::InvalidData.into())
                }
                Ok(n) => {
                    if self.bytes.len() + n > LIMIT {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    self.bytes.extend_from_slice(&b[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }
}
pub struct Writer {
    pub pipe: NativePipe,
    bytes: Vec<u8>,
    offset: usize,
    deadline: Instant,
}
impl Writer {
    pub fn new(pipe: NativePipe, text: &str) -> Self {
        Self {
            pipe,
            bytes: text.as_bytes().to_vec(),
            offset: 0,
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }
    pub fn interest(&self, wait: &mut HostWait) -> io::Result<()> {
        wait.watch(&self.pipe, true)?;
        wait.until(self.deadline);
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<bool> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        while self.offset < self.bytes.len() {
            match self.pipe.write(&self.bytes[self.offset..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }
}
