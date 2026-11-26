// SPDX-License-Identifier: GPL-3.0-or-later
//! ICCCM/EWMH requests on the existing native input connection.
use std::io;
use trames_wayland::{WindowSnapshot, WindowState};
use x11rb::{protocol::xproto::*, wrapper::ConnectionExt as _, xcb_ffi::XCBConnection};
fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub struct Manager {
    pub state: u32,
    pub wm_state: u32,
    max_h: u32,
    max_v: u32,
    full: u32,
    hidden: u32,
    change: u32,
    moveresize: u32,
    name: u32,
    utf8: u32,
}
impl Manager {
    pub fn new(c: &XCBConnection) -> io::Result<Self> {
        let atom = |s: &[u8]| -> io::Result<u32> {
            Ok(c.intern_atom(false, s)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .atom)
        };
        Ok(Self {
            state: atom(b"_NET_WM_STATE")?,
            wm_state: atom(b"WM_STATE")?,
            max_h: atom(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
            max_v: atom(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
            full: atom(b"_NET_WM_STATE_FULLSCREEN")?,
            hidden: atom(b"_NET_WM_STATE_HIDDEN")?,
            change: atom(b"WM_CHANGE_STATE")?,
            moveresize: atom(b"_NET_WM_MOVERESIZE")?,
            name: atom(b"_NET_WM_NAME")?,
            utf8: atom(b"UTF8_STRING")?,
        })
    }
    fn message(
        &self,
        c: &XCBConnection,
        root: u32,
        id: u32,
        kind: u32,
        data: [u32; 5],
    ) -> io::Result<()> {
        c.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ClientMessageEvent::new(32, id, kind, data),
        )
        .map_err(err)?;
        Ok(())
    }
    pub fn read(&self, c: &XCBConnection, id: u32) -> io::Result<WindowState> {
        let p = c
            .get_property(false, id, self.state, AtomEnum::ATOM, 0, 64)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let atoms = p
            .value32()
            .map(|v| v.collect::<Vec<_>>())
            .unwrap_or_default();
        let p = c
            .get_property(false, id, self.wm_state, AtomEnum::ANY, 0, 2)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        Ok(WindowState {
            maximized: atoms.contains(&self.max_h) && atoms.contains(&self.max_v),
            fullscreen: atoms.contains(&self.full),
            minimized: atoms.contains(&self.hidden)
                || p.value32().and_then(|mut v| v.next()) == Some(3),
            resizing: false,
        })
    }
    pub fn state(
        &self,
        c: &XCBConnection,
        root: u32,
        id: u32,
        old: Option<WindowState>,
        next: WindowState,
        fields: u8,
    ) -> io::Result<()> {
        if old.is_some() {
            if fields & 1 != 0 {
                self.message(
                    c,
                    root,
                    id,
                    self.state,
                    [u32::from(next.maximized), self.max_h, self.max_v, 1, 0],
                )?;
            }
            if fields & 2 != 0 {
                self.message(
                    c,
                    root,
                    id,
                    self.state,
                    [u32::from(next.fullscreen), self.full, 0, 1, 0],
                )?;
            }
        } else {
            let mut atoms = vec![];
            if next.maximized {
                atoms.extend([self.max_h, self.max_v]);
            }
            if next.fullscreen {
                atoms.push(self.full);
            }
            c.change_property32(PropMode::REPLACE, id, self.state, AtomEnum::ATOM, &atoms)
                .map_err(err)?;
        }
        if fields & 4 != 0 && next.minimized {
            self.message(c, root, id, self.change, [3, 0, 0, 0, 0])?;
        } else if fields & 4 != 0 && old.is_some_and(|s| s.minimized) {
            c.map_window(id).map_err(err)?;
        }
        Ok(())
    }
    pub fn metadata(
        &self,
        c: &XCBConnection,
        id: u32,
        scene: &WindowSnapshot,
        parent: Option<u32>,
    ) -> io::Result<()> {
        c.change_property8(
            PropMode::REPLACE,
            id,
            self.name,
            self.utf8,
            scene.title.as_bytes(),
        )
        .map_err(err)?;
        if let Some(parent) = parent {
            c.change_property32(
                PropMode::REPLACE,
                id,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                &[parent],
            )
            .map_err(err)?;
        } else {
            c.delete_property(id, AtomEnum::WM_TRANSIENT_FOR.into())
                .map_err(err)?;
        }
        let mut hints = [0u32; 18];
        let bounds = scene.bounds();
        let extra = (
            bounds[2].saturating_sub(scene.width as i32).max(0) as u32,
            bounds[3].saturating_sub(scene.height as i32).max(0) as u32,
        );
        if scene.min_size != (0, 0) {
            hints[0] |= 1 << 4;
            hints[5] = scene.min_size.0.saturating_add(extra.0).min(4096);
            hints[6] = scene.min_size.1.saturating_add(extra.1).min(4096);
        }
        if scene.max_size != (0, 0) {
            hints[0] |= 1 << 5;
            hints[7] = if scene.max_size.0 == 0 {
                4096
            } else {
                scene
                    .max_size
                    .0
                    .max(scene.min_size.0)
                    .saturating_add(extra.0)
                    .min(4096)
            };
            hints[8] = if scene.max_size.1 == 0 {
                4096
            } else {
                scene
                    .max_size
                    .1
                    .max(scene.min_size.1)
                    .saturating_add(extra.1)
                    .min(4096)
            };
        }
        c.change_property32(
            PropMode::REPLACE,
            id,
            AtomEnum::WM_NORMAL_HINTS,
            AtomEnum::WM_SIZE_HINTS,
            &hints,
        )
        .map_err(err)?;
        Ok(())
    }
    pub fn interact(
        &self,
        c: &XCBConnection,
        root: u32,
        id: u32,
        edges: u32,
        press: (i16, i16, u8, u32),
    ) -> io::Result<()> {
        let direction = match edges {
            5 => 0,
            1 => 1,
            9 => 2,
            8 => 3,
            10 => 4,
            2 => 5,
            6 => 6,
            4 => 7,
            0 => 8,
            _ => return Ok(()),
        };
        c.ungrab_pointer(press.3).map_err(err)?;
        self.message(
            c,
            root,
            id,
            self.moveresize,
            [
                press.0 as i32 as u32,
                press.1 as i32 as u32,
                direction,
                press.2.into(),
                1,
            ],
        )
    }
}
