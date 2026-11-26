// SPDX-License-Identifier: GPL-3.0-or-later
//! Export the original X server's XKB map; state comes from its ordered XKB
//! events. Only map/control changes make native queries, never ordinary keys.
use std::{
    ffi::{c_void, CStr},
    io,
    sync::Arc,
};
use trames_wayland::KeyboardModifiers;
use x11rb::{
    protocol::{
        xkb::{self, ConnectionExt as _},
        Event,
    },
    xcb_ffi::XCBConnection,
};
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("host XKB: {e}"))
}
type Unref = unsafe extern "C" fn(*mut c_void);
pub enum Update {
    Map(Arc<[u8]>),
    Repeat(i32, i32),
    Modifiers(KeyboardModifiers),
}
pub struct Keyboard {
    _common: libloading::Library,
    _x11: libloading::Library,
    context: *mut c_void,
    context_unref: Unref,
    map_unref: Unref,
    new_map: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, u32) -> *mut c_void,
    map_string: unsafe extern "C" fn(*mut c_void, u32) -> *mut std::ffi::c_char,
    device: i32,
    pub modifiers: KeyboardModifiers,
}
impl Keyboard {
    pub fn new(c: &XCBConnection) -> io::Result<Self> {
        let extension = c
            .xkb_use_extension(1, 0)
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if !extension.supported {
            return Err(error("XKB extension is required"));
        }
        // SAFETY: fixed system library names and the stable public xkbcommon C ABI.
        let common = unsafe { libloading::Library::new("libxkbcommon.so.0") }.map_err(error)?;
        let x11 = unsafe { libloading::Library::new("libxkbcommon-x11.so.0") }.map_err(error)?;
        let (context, context_unref, map_unref, new_map, map_string, device) = unsafe {
            let new_context: libloading::Symbol<unsafe extern "C" fn(u32) -> *mut c_void> =
                common.get(b"xkb_context_new\0").map_err(error)?;
            let context_unref = *common.get::<Unref>(b"xkb_context_unref\0").map_err(error)?;
            let map_unref = *common.get::<Unref>(b"xkb_keymap_unref\0").map_err(error)?;
            let new_map = *x11
                .get::<unsafe extern "C" fn(*mut c_void, *mut c_void, i32, u32) -> *mut c_void>(
                    b"xkb_x11_keymap_new_from_device\0",
                )
                .map_err(error)?;
            let map_string = *common
                .get::<unsafe extern "C" fn(*mut c_void, u32) -> *mut std::ffi::c_char>(
                    b"xkb_keymap_get_as_string\0",
                )
                .map_err(error)?;
            let get_device = x11
                .get::<unsafe extern "C" fn(*mut c_void) -> i32>(
                    b"xkb_x11_get_core_keyboard_device_id\0",
                )
                .map_err(error)?;
            let device = get_device(c.get_raw_xcb_connection());
            let context = new_context(0);
            (
                context,
                context_unref,
                map_unref,
                new_map,
                map_string,
                device,
            )
        };
        if context.is_null() {
            return Err(error("cannot allocate XKB context"));
        }
        let mut k = Self {
            _common: common,
            _x11: x11,
            context,
            context_unref,
            map_unref,
            new_map,
            map_string,
            device,
            modifiers: KeyboardModifiers::default(),
        };
        if device < 0 {
            return Err(error("missing core keyboard"));
        }
        c.xkb_select_events(
            xkb::ID::USE_CORE_KBD.into(),
            0u16.into(),
            xkb::EventType::NEW_KEYBOARD_NOTIFY
                | xkb::EventType::MAP_NOTIFY
                | xkb::EventType::STATE_NOTIFY
                | xkb::EventType::CONTROLS_NOTIFY,
            0xffu16.into(),
            0xffu16.into(),
            &xkb::SelectEventsAux::default(),
        )
        .map_err(error)?
        .check()
        .map_err(error)?;
        let flags = c
            .xkb_per_client_flags(
                xkb::ID::USE_CORE_KBD.into(),
                xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                0u32.into(),
                0u32.into(),
                0u32.into(),
            )
            .map_err(error)?
            .reply()
            .map_err(error)?;
        if !flags
            .value
            .contains(xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT)
        {
            return Err(error("detectable autorepeat is required"));
        }
        let s = c
            .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
            .map_err(error)?
            .reply()
            .map_err(error)?;
        k.modifiers = KeyboardModifiers {
            depressed: u16::from(s.base_mods) as u32,
            latched: u16::from(s.latched_mods) as u32,
            locked: u16::from(s.locked_mods) as u32,
            group: u8::from(s.group) as u32,
        };
        Ok(k)
    }
    pub fn map(&self, c: &XCBConnection) -> io::Result<Arc<[u8]>> {
        // SAFETY: context/connection are live; returned map and malloc string are
        // owned native resources, released after making one immutable byte copy.
        unsafe {
            let map = (self.new_map)(self.context, c.get_raw_xcb_connection(), self.device, 0);
            if map.is_null() {
                return Err(error("cannot export keymap"));
            }
            let text = (self.map_string)(map, 1);
            (self.map_unref)(map);
            if text.is_null() {
                return Err(error("cannot serialize keymap"));
            }
            let bytes = CStr::from_ptr(text).to_bytes_with_nul().to_vec();
            libc::free(text.cast());
            Ok(bytes.into())
        }
    }
    pub fn repeat(&self, c: &XCBConnection) -> io::Result<(i32, i32)> {
        let r = c
            .xkb_get_controls(xkb::ID::USE_CORE_KBD.into())
            .map_err(error)?
            .reply()
            .map_err(error)?;
        let rate =
            if r.enabled_controls.contains(xkb::BoolCtrl::REPEAT_KEYS) && r.repeat_interval > 0 {
                1000 / i32::from(r.repeat_interval)
            } else {
                0
            };
        Ok((rate, i32::from(r.repeat_delay)))
    }
    pub fn event(&mut self, c: &XCBConnection, e: &Event) -> io::Result<Option<Vec<Update>>> {
        Ok(match e {
            Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                let (r, d) = self.repeat(c)?;
                Some(vec![Update::Map(self.map(c)?), Update::Repeat(r, d)])
            }
            Event::XkbControlsNotify(_) => {
                let (r, d) = self.repeat(c)?;
                Some(vec![Update::Repeat(r, d)])
            }
            Event::XkbStateNotify(s) => {
                self.modifiers = KeyboardModifiers {
                    depressed: u16::from(s.base_mods) as u32,
                    latched: u16::from(s.latched_mods) as u32,
                    locked: u16::from(s.locked_mods) as u32,
                    group: u8::from(s.group) as u32,
                };
                Some(vec![Update::Modifiers(self.modifiers)])
            }
            _ => None,
        })
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe {
            (self.context_unref)(self.context);
        }
    }
}
