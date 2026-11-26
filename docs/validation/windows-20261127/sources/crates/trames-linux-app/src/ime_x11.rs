// SPDX-License-Identifier: GPL-3.0-or-later
//! XIM client on the original XCB event connection. The host IM owns candidates;
//! composition uses the shared UTF-8 context/epoch contract.
use std::{collections::VecDeque, io, sync::Arc};
use trames_wayland::{TextContext, TextEdit};
use x11rb::{
    protocol::{
        xproto::{KeyPressEvent, KEY_PRESS_EVENT},
        Event,
    },
    xcb_ffi::XCBConnection,
};
use xim::{
    x11rb::X11rbClient, AttributeName, Client, ClientError, ClientHandler, ForwardEventFlag,
    InputStyle, Point,
};
type Native = X11rbClient<Arc<XCBConnection>>;
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
#[derive(Default)]
struct Handler {
    im: u16,
    ic: u16,
    active: Option<(u32, TextContext)>,
    creating: Option<(u32, TextContext)>,
    preedit: String,
    edits: VecDeque<TextEdit>,
    forwarded: VecDeque<KeyPressEvent>,
    pending: VecDeque<KeyPressEvent>,
    fallback: bool,
}
impl Handler {
    fn stamp(&self) -> Option<TextEdit> {
        let c = &self.active.as_ref()?.1;
        Some(TextEdit {
            context: c.id,
            epoch: c.epoch,
            serial: c.serial,
            ..Default::default()
        })
    }
    fn preedit(&mut self, caret: usize) {
        if let Some(mut edit) = self.stamp() {
            let p = self
                .preedit
                .char_indices()
                .nth(caret)
                .map_or(self.preedit.len(), |(p, _)| p) as i32;
            edit.preedit = Some((self.preedit.clone(), p, p));
            self.edits.push_back(edit);
        }
    }
    fn create(&mut self, c: &mut Native) -> Result<(), ClientError> {
        if self.im == 0 || self.ic != 0 || self.creating.is_some() {
            return Ok(());
        }
        let Some((window, context)) = self.active.clone() else {
            return Ok(());
        };
        let style = if self.fallback {
            InputStyle::PREEDIT_NOTHING
        } else if context.version == 4 {
            InputStyle::PREEDIT_POSITION
        } else {
            InputStyle::PREEDIT_CALLBACKS
        };
        let attrs = c
            .build_ic_attributes()
            .push(
                AttributeName::InputStyle,
                style | InputStyle::STATUS_NOTHING,
            )
            .push(AttributeName::ClientWindow, window)
            .push(AttributeName::FocusWindow, window)
            .nested_list(AttributeName::PreeditAttributes, |b| {
                b.push(
                    AttributeName::SpotLocation,
                    Point {
                        x: context.caret[0].clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                        y: context.caret[1].clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    },
                );
            })
            .build();
        c.create_ic(self.im, attrs)?;
        self.creating = Some((window, context));
        Ok(())
    }
}
impl ClientHandler<Native> for Handler {
    fn handle_connect(&mut self, c: &mut Native) -> Result<(), ClientError> {
        let locale = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| "C.UTF-8".into());
        c.open(&locale)
    }
    fn handle_open(&mut self, c: &mut Native, im: u16) -> Result<(), ClientError> {
        self.im = im;
        self.create(c)
    }
    fn handle_create_ic(&mut self, c: &mut Native, im: u16, ic: u16) -> Result<(), ClientError> {
        let pending = self.creating.take();
        if pending
            .as_ref()
            .zip(self.active.as_ref())
            .is_some_and(|(p, a)| p.0 == a.0 && p.1.id == a.1.id && p.1.epoch == a.1.epoch)
        {
            self.ic = ic;
            c.set_focus(im, ic)?;
            while let Some(key) = self.pending.pop_front() {
                c.forward_event(im, ic, ForwardEventFlag::empty(), &key)?;
            }
        } else {
            c.destroy_ic(im, ic)?;
            self.create(c)?;
        }
        Ok(())
    }
    fn handle_commit(
        &mut self,
        _: &mut Native,
        _: u16,
        ic: u16,
        text: &str,
    ) -> Result<(), ClientError> {
        if ic == self.ic && text.len() <= 4096 {
            if let Some(mut edit) = self.stamp() {
                edit.preedit = Some((String::new(), 0, 0));
                edit.commit = Some(text.into());
                self.edits.push_back(edit);
            }
            self.preedit.clear();
        }
        Ok(())
    }
    fn handle_forward_event(
        &mut self,
        _: &mut Native,
        _: u16,
        ic: u16,
        _: ForwardEventFlag,
        key: KeyPressEvent,
    ) -> Result<(), ClientError> {
        if ic == self.ic {
            self.forwarded.push_back(key);
        }
        Ok(())
    }
    fn handle_preedit_start(&mut self, _: &mut Native, _: u16, ic: u16) -> Result<(), ClientError> {
        if ic == self.ic {
            self.preedit.clear();
        }
        Ok(())
    }
    fn handle_preedit_done(&mut self, _: &mut Native, _: u16, ic: u16) -> Result<(), ClientError> {
        if ic == self.ic {
            self.preedit.clear();
            self.preedit(0);
        }
        Ok(())
    }
    fn handle_preedit_draw(
        &mut self,
        _: &mut Native,
        _: u16,
        ic: u16,
        caret: i32,
        first: i32,
        length: i32,
        _: xim::PreeditDrawStatus,
        text: &str,
        _: Vec<xim::Feedback>,
    ) -> Result<(), ClientError> {
        if ic != self.ic || first < 0 || length < 0 {
            return Ok(());
        }
        let old: Vec<_> = self.preedit.chars().collect();
        let (a, b) = (
            first as usize,
            (first as usize).saturating_add(length as usize),
        );
        if b > old.len() {
            return Ok(());
        }
        let result: String = old[..a]
            .iter()
            .copied()
            .chain(text.chars())
            .chain(old[b..].iter().copied())
            .collect();
        if result.len() <= 4096 {
            self.preedit = result;
            self.preedit(caret.max(0) as usize);
        }
        Ok(())
    }
    fn handle_disconnect(&mut self) {
        self.im = 0;
        self.ic = 0;
        self.creating = None;
        self.forwarded.append(&mut self.pending);
        self.preedit.clear();
    }
}
pub struct Ime {
    connection: Arc<XCBConnection>,
    screen: usize,
    client: Option<Native>,
    handler: Handler,
    root: u32,
    servers: u32,
}
impl Ime {
    pub fn new(connection: Arc<XCBConnection>, screen: usize) -> io::Result<Self> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt, EventMask};
        let root = connection.setup().roots[screen].root;
        let servers = connection
            .intern_atom(false, b"XIM_SERVERS")
            .map_err(error)?
            .reply()
            .map_err(error)?
            .atom;
        connection
            .change_window_attributes(
                root,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map_err(error)?;
        let client = Native::init(connection.clone(), screen, None).ok();
        Ok(Self {
            connection,
            screen,
            client,
            handler: Handler::default(),
            root,
            servers,
        })
    }
    pub fn event(&mut self, e: &Event) -> io::Result<bool> {
        if matches!(e,Event::PropertyNotify(p) if p.window==self.root&&p.atom==self.servers)
            && self.client.is_none()
        {
            self.client = Native::init(self.connection.clone(), self.screen, None).ok();
        }
        if let Some(c) = &mut self.client {
            match c.filter_event(e, &mut self.handler) {
                Ok(used) => return Ok(used),
                Err(_) => {
                    if self.handler.creating.take().is_some() && !self.handler.fallback {
                        self.handler.fallback = true;
                        self.handler.create(c).map_err(error)?;
                    } else {
                        self.handler.handle_disconnect();
                        self.client = None;
                    }
                }
            }
        }
        Ok(false)
    }
    pub fn sync(&mut self, context: Option<&TextContext>, native: Option<u32>) -> io::Result<()> {
        let active = context.zip(native).map(|(c, w)| (w, c.clone()));
        let old = self
            .handler
            .active
            .as_ref()
            .map(|(w, c)| (*w, c.id, c.epoch));
        let new = active.as_ref().map(|(w, c)| (*w, c.id, c.epoch));
        if old != new {
            if let Some(c) = &mut self.client {
                if self.handler.ic != 0 {
                    c.unset_focus(self.handler.im, self.handler.ic)
                        .map_err(error)?;
                    c.destroy_ic(self.handler.im, self.handler.ic)
                        .map_err(error)?;
                }
            }
            self.handler.ic = 0;
            self.handler.pending.clear();
            self.handler.preedit.clear();
            self.handler.fallback = false;
        }
        let changed = self.handler.active.as_ref().map(|(_, c)| c.serial)
            != active.as_ref().map(|(_, c)| c.serial);
        self.handler.active = active;
        if let Some(c) = &mut self.client {
            self.handler.create(c).map_err(error)?;
            if changed && self.handler.ic != 0 {
                if let Some((_, context)) = &self.handler.active {
                    let attrs = c
                        .build_ic_attributes()
                        .nested_list(AttributeName::PreeditAttributes, |b| {
                            b.push(
                                AttributeName::SpotLocation,
                                Point {
                                    x: context.caret[0].clamp(i16::MIN as i32, i16::MAX as i32)
                                        as i16,
                                    y: context.caret[1].clamp(i16::MIN as i32, i16::MAX as i32)
                                        as i16,
                                },
                            );
                        })
                        .build();
                    c.set_ic_values(self.handler.im, self.handler.ic, attrs)
                        .map_err(error)?;
                }
            }
        }
        Ok(())
    }
    pub fn forward(&mut self, key: &KeyPressEvent) -> io::Result<bool> {
        if self.handler.active.is_none() || self.client.is_none() {
            return Ok(false);
        }
        if self.handler.ic == 0 {
            if self.handler.pending.len() < 256 {
                self.handler.pending.push_back(*key);
            }
            return Ok(true);
        }
        self.client
            .as_mut()
            .unwrap()
            .forward_event(
                self.handler.im,
                self.handler.ic,
                ForwardEventFlag::empty(),
                key,
            )
            .map_err(error)?;
        Ok(true)
    }
    pub fn key(&mut self) -> Option<Event> {
        self.handler.forwarded.pop_front().map(|k| {
            if k.response_type & 0x7f == KEY_PRESS_EVENT {
                Event::KeyPress(k)
            } else {
                Event::KeyRelease(k)
            }
        })
    }
    pub fn edits(&mut self) -> impl Iterator<Item = TextEdit> + '_ {
        self.handler.edits.drain(..)
    }
}
