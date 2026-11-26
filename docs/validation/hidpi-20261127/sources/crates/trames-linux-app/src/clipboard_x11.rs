// SPDX-License-Identifier: GPL-3.0-or-later
use crate::clipboard::{Bridge, Reader, Request, Writer, LIMIT};
use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use trames_shared::{HostWait, NativePipe};
use x11rb::{
    connection::Connection,
    protocol::{
        xfixes::{self, ConnectionExt as _},
        xproto::{
            self, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, WindowClass,
        },
        Event,
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE,
};
fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
struct Incoming {
    pipe: NativePipe,
    bytes: Vec<u8>,
    incr: bool,
    target: u32,
    deadline: Instant,
}
pub struct Clipboard {
    pub bridge: Arc<Bridge>,
    window: u32,
    selection: u32,
    utf8: u32,
    targets: u32,
    text: u32,
    string: u32,
    incr: u32,
    timestamp: u32,
    incoming: BTreeMap<u32, Incoming>,
    readers: Vec<Reader>,
    writers: Vec<Writer>,
    owned: Option<String>,
    properties: Vec<u32>,
    time: u32,
    changed: bool,
}
impl Clipboard {
    pub fn new(
        c: &XCBConnection,
        screen: usize,
        server: &trames_wayland::Server,
    ) -> io::Result<Self> {
        let atom = |s: &[u8]| -> io::Result<u32> {
            Ok(c.intern_atom(false, s)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .atom)
        };
        let selection = atom(b"CLIPBOARD")?;
        let window = c.generate_id().map_err(err)?;
        c.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            c.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(err)?
        .check()
        .map_err(err)?;
        c.xfixes_query_version(5, 0)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        c.xfixes_select_selection_input(
            window,
            selection,
            xfixes::SelectionEventMask::SET_SELECTION_OWNER
                | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(err)?
        .check()
        .map_err(err)?;
        let bridge = Arc::new(Bridge::default());
        server.attach_clipboard(bridge.clone())?;
        Ok(Self {
            bridge,
            window,
            selection,
            utf8: atom(b"UTF8_STRING")?,
            targets: atom(b"TARGETS")?,
            text: atom(b"TEXT")?,
            string: u32::from(AtomEnum::STRING),
            incr: atom(b"INCR")?,
            timestamp: atom(b"TIMESTAMP")?,
            incoming: BTreeMap::new(),
            readers: vec![],
            writers: vec![],
            owned: None,
            properties: (0..16)
                .map(|i| atom(format!("_TRAMES_CLIPBOARD_{i}").as_bytes()))
                .collect::<io::Result<_>>()?,
            time: 0,
            changed: false,
        })
    }
    pub fn interest(&self, w: &mut HostWait, server: &trames_wayland::Server) -> io::Result<()> {
        self.bridge.arm(w);
        for r in &self.readers {
            r.interest(w)?;
        }
        for r in &self.writers {
            r.interest(w)?;
        }
        for r in self.incoming.values() {
            w.until(r.deadline);
        }
        if self.changed {
            w.retain([server.subscribe_commands(w.signal())]);
        }
        Ok(())
    }
    pub fn poll(&mut self, c: &XCBConnection, server: &trames_wayland::Server) -> io::Result<()> {
        for r in self.bridge.take() {
            if self.incoming.len() + self.readers.len() + self.writers.len() >= 16 {
                continue;
            }
            match r {
                Request::Write(p) => self.readers.push(Reader::new(p)),
                Request::Read(p) => {
                    if let Some(text) = &self.owned {
                        self.writers.push(Writer::new(p, text));
                    } else if c
                        .get_selection_owner(self.selection)
                        .map_err(err)?
                        .reply()
                        .map_err(err)?
                        .owner
                        != NONE
                    {
                        let Some(property) = self
                            .properties
                            .iter()
                            .find(|p| !self.incoming.contains_key(p))
                            .copied()
                        else {
                            continue;
                        };
                        c.convert_selection(
                            self.window,
                            self.selection,
                            self.targets,
                            property,
                            CURRENT_TIME,
                        )
                        .map_err(err)?;
                        self.incoming.insert(
                            property,
                            Incoming {
                                pipe: p,
                                bytes: vec![],
                                incr: false,
                                target: self.targets,
                                deadline: Instant::now() + Duration::from_secs(30),
                            },
                        );
                    }
                }
            }
        }
        let mut n = 0;
        while n < self.readers.len() {
            match self.readers[n].poll() {
                Ok(None) => n += 1,
                Ok(Some(text)) => {
                    self.owned = Some(text);
                    self.readers.swap_remove(n);
                    c.set_selection_owner(self.window, self.selection, CURRENT_TIME)
                        .map_err(err)?;
                }
                Err(_) => {
                    self.readers.swap_remove(n);
                }
            }
        }
        self.writers.retain_mut(|w| matches!(w.poll(), Ok(false)));
        self.incoming.retain(|_, v| v.deadline > Instant::now());
        if self.changed {
            match server.clipboard_changed() {
                Ok(()) => self.changed = false,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn property(&mut self, c: &XCBConnection, property: u32) -> io::Result<()> {
        let Some(transfer) = self.incoming.get_mut(&property) else {
            return Ok(());
        };
        let reply = c
            .get_property(
                true,
                self.window,
                property,
                AtomEnum::ANY,
                0,
                (LIMIT / 4 + 1) as u32,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if transfer.target == self.targets {
            let types = reply
                .value32()
                .map(|v| v.collect::<Vec<_>>())
                .unwrap_or_default();
            let target = [self.utf8, self.text, self.string]
                .into_iter()
                .find(|t| types.contains(t));
            if let Some(target) = target {
                transfer.target = target;
                c.convert_selection(self.window, self.selection, target, property, CURRENT_TIME)
                    .map_err(err)?;
            } else {
                self.incoming.remove(&property);
            }
            return Ok(());
        }
        if reply.type_ == self.incr {
            transfer.incr = true;
            return Ok(());
        }
        if reply.bytes_after != 0 || transfer.bytes.len() + reply.value.len() > LIMIT {
            self.incoming.remove(&property);
            return Ok(());
        }
        if reply.format != 8 {
            self.incoming.remove(&property);
            return Ok(());
        }
        let finished = !transfer.incr || reply.value.is_empty();
        transfer.bytes.extend_from_slice(&reply.value);
        if finished {
            let transfer = self.incoming.remove(&property).unwrap();
            let text = if transfer.target == self.string {
                Ok(transfer.bytes.into_iter().map(char::from).collect())
            } else {
                String::from_utf8(transfer.bytes)
            };
            if let Ok(text) = text {
                self.writers.push(Writer::new(transfer.pipe, &text));
            }
        }
        Ok(())
    }
    pub fn event(&mut self, c: &XCBConnection, event: &Event) -> io::Result<bool> {
        match event {
            Event::XfixesSelectionNotify(e) if e.selection == self.selection => {
                self.time = e.selection_timestamp;
                if e.owner != self.window {
                    self.owned = None;
                    self.changed = true;
                }
                Ok(true)
            }
            Event::SelectionClear(e) if e.owner == self.window && e.selection == self.selection => {
                self.owned = None;
                Ok(true)
            }
            Event::SelectionNotify(e)
                if e.requestor == self.window && e.selection == self.selection =>
            {
                if e.property != NONE {
                    self.property(c, e.property)?;
                } else {
                    let failed: Vec<_> = self
                        .incoming
                        .iter()
                        .filter(|(_, t)| t.target == e.target)
                        .map(|(p, _)| *p)
                        .collect();
                    for property in failed {
                        if e.target == self.targets {
                            self.incoming.get_mut(&property).unwrap().target = self.utf8;
                            c.convert_selection(
                                self.window,
                                self.selection,
                                self.utf8,
                                property,
                                CURRENT_TIME,
                            )
                            .map_err(err)?;
                        } else {
                            self.incoming.remove(&property);
                        }
                    }
                }
                Ok(true)
            }
            Event::PropertyNotify(e)
                if e.window == self.window
                    && e.state == xproto::Property::NEW_VALUE
                    && self.incoming.get(&e.atom).is_some_and(|i| i.incr) =>
            {
                self.property(c, e.atom)?;
                Ok(true)
            }
            Event::SelectionRequest(e)
                if e.owner == self.window && e.selection == self.selection =>
            {
                let property = if e.property == NONE {
                    e.target
                } else {
                    e.property
                };
                let mut accepted = NONE;
                if let Some(text) = &self.owned {
                    if e.target == self.targets {
                        c.change_property32(
                            PropMode::REPLACE,
                            e.requestor,
                            property,
                            AtomEnum::ATOM,
                            &[
                                self.targets,
                                self.utf8,
                                self.text,
                                self.string,
                                self.timestamp,
                            ],
                        )
                        .map_err(err)?
                        .check()
                        .or_else(|e| match e {
                            x11rb::errors::ReplyError::X11Error(e)
                                if e.error_kind == x11rb::protocol::ErrorKind::Window =>
                            {
                                Ok(())
                            }
                            e => Err(e),
                        })
                        .map_err(err)?;
                        accepted = property;
                    } else if e.target == self.timestamp {
                        c.change_property32(
                            PropMode::REPLACE,
                            e.requestor,
                            property,
                            AtomEnum::INTEGER,
                            &[self.time],
                        )
                        .map_err(err)?
                        .check()
                        .or_else(|e| match e {
                            x11rb::errors::ReplyError::X11Error(e)
                                if e.error_kind == x11rb::protocol::ErrorKind::Window =>
                            {
                                Ok(())
                            }
                            e => Err(e),
                        })
                        .map_err(err)?;
                        accepted = property;
                    } else if e.target == self.utf8 || e.target == self.text {
                        c.change_property8(
                            PropMode::REPLACE,
                            e.requestor,
                            property,
                            self.utf8,
                            text.as_bytes(),
                        )
                        .map_err(err)?
                        .check()
                        .or_else(|e| match e {
                            x11rb::errors::ReplyError::X11Error(e)
                                if e.error_kind == x11rb::protocol::ErrorKind::Window =>
                            {
                                Ok(())
                            }
                            e => Err(e),
                        })
                        .map_err(err)?;
                        accepted = property;
                    } else if e.target == self.string && text.chars().all(|c| u32::from(c) <= 255) {
                        let bytes: Vec<_> = text.chars().map(|c| c as u8).collect();
                        c.change_property8(
                            PropMode::REPLACE,
                            e.requestor,
                            property,
                            self.string,
                            &bytes,
                        )
                        .map_err(err)?
                        .check()
                        .or_else(|e| match e {
                            x11rb::errors::ReplyError::X11Error(e)
                                if e.error_kind == x11rb::protocol::ErrorKind::Window =>
                            {
                                Ok(())
                            }
                            e => Err(e),
                        })
                        .map_err(err)?;
                        accepted = property;
                    }
                }
                let reply = xproto::SelectionNotifyEvent {
                    response_type: xproto::SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: e.time,
                    requestor: e.requestor,
                    selection: e.selection,
                    target: e.target,
                    property: accepted,
                };
                c.send_event(false, e.requestor, EventMask::NO_EVENT, reply)
                    .map_err(err)?
                    .check()
                    .or_else(|e| match e {
                        x11rb::errors::ReplyError::X11Error(e)
                            if e.error_kind == x11rb::protocol::ErrorKind::Window =>
                        {
                            Ok(())
                        }
                        e => Err(e),
                    })
                    .map_err(err)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
