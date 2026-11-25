// SPDX-License-Identifier: GPL-3.0-or-later
//! Host window capability and events on the existing foreground host loop.
#[cfg(target_os = "linux")]
mod x11 {
    use std::{io, sync::Arc};
    use x11rb::{
        connection::Connection,
        protocol::{
            xproto::{AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, WindowClass},
            Event,
        },
        wrapper::ConnectionExt as _,
        xcb_ffi::XCBConnection,
        COPY_DEPTH_FROM_PARENT,
    };
    struct Window {
        // Vulkan's private connection cannot drain/buffer host input events and
        // hide their readiness from the original foreground HostWait.
        events: XCBConnection,
        native: XCBConnection,
        id: u32,
    }
    impl Drop for Window {
        fn drop(&mut self) {
            let _ = self.events.destroy_window(self.id);
            let _ = self.events.flush();
        }
    }
    pub struct HostDisplay {
        window: Arc<Window>,
        display: trames_session::VulkanDisplay,
        protocols: u32,
        close: u32,
        dimensions: (u32, u32),
        closed: bool,
    }
    impl HostDisplay {
        pub fn open(
            display: trames_session::VulkanDisplay,
            width: u32,
            height: u32,
        ) -> io::Result<Self> {
            if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                return Err(io::Error::other("display dimensions must be 1..4096"));
            }
            x11rb::xcb_ffi::load_libxcb().map_err(io::Error::other)?;
            let (events, screen) = XCBConnection::connect(None).map_err(io::Error::other)?;
            let (native, _) = XCBConnection::connect(None).map_err(io::Error::other)?;
            let screen = events
                .setup()
                .roots
                .get(screen)
                .ok_or_else(|| io::Error::other("missing X11 screen"))?;
            let id = events.generate_id().map_err(io::Error::other)?;
            events
                .create_window(
                    COPY_DEPTH_FROM_PARENT,
                    id,
                    screen.root,
                    0,
                    0,
                    width as u16,
                    height as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new()
                        .background_pixel(screen.black_pixel)
                        .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::EXPOSURE),
                )
                .map_err(io::Error::other)?
                .check()
                .map_err(io::Error::other)?;
            let window = Arc::new(Window { events, native, id });
            let events = &window.events;
            let atom = |name: &[u8]| -> io::Result<u32> {
                Ok(events
                    .intern_atom(false, name)
                    .map_err(io::Error::other)?
                    .reply()
                    .map_err(io::Error::other)?
                    .atom)
            };
            let protocols = atom(b"WM_PROTOCOLS")?;
            let close = atom(b"WM_DELETE_WINDOW")?;
            events
                .change_property32(PropMode::REPLACE, id, protocols, AtomEnum::ATOM, &[close])
                .map_err(io::Error::other)?
                .check()
                .map_err(io::Error::other)?;
            events
                .change_property8(
                    PropMode::REPLACE,
                    id,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    b"Trames Linux",
                )
                .map_err(io::Error::other)?
                .check()
                .map_err(io::Error::other)?;
            events
                .map_window(id)
                .map_err(io::Error::other)?
                .check()
                .map_err(io::Error::other)?;
            events.flush().map_err(io::Error::other)?;
            let host = Self {
                window,
                display,
                protocols,
                close,
                dimensions: (width, height),
                closed: false,
            };
            host.attach(width, height)?;
            Ok(host)
        }
        fn attach(&self, width: u32, height: u32) -> io::Result<()> {
            // SAFETY: Window retains both connections and its XID. The checked
            // creation/map requests completed before another connection uses it.
            // libxcb is thread-safe; only Vulkan uses its native connection.
            unsafe {
                self.display.attach_xcb(
                    self.window.native.get_raw_xcb_connection(),
                    self.window.id,
                    width,
                    height,
                    self.window.clone(),
                )
            }
            .map_err(io::Error::other)
        }
        pub fn closed(&self) -> bool {
            self.closed
        }
        pub fn interest(&self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
            use std::os::fd::AsFd;
            wait.watch(self.window.events.as_fd(), false)
        }
        pub fn poll(&mut self) -> io::Result<()> {
            while let Some(event) = self
                .window
                .events
                .poll_for_event()
                .map_err(io::Error::other)?
            {
                match event {
                    Event::ClientMessage(event)
                        if event.window == self.window.id
                            && event.type_ == self.protocols
                            && event.format == 32
                            && event.data.as_data32()[0] == self.close =>
                    {
                        self.closed = true
                    }
                    Event::DestroyNotify(event) if event.window == self.window.id => {
                        self.closed = true
                    }
                    Event::ConfigureNotify(event) if event.window == self.window.id => {
                        let size = (u32::from(event.width), u32::from(event.height));
                        if size != self.dimensions {
                            self.attach(size.0, size.1)?;
                            self.dimensions = size;
                        }
                    }
                    _ => (),
                }
            }
            if self.closed {
                self.display.detach();
            }
            Ok(())
        }
    }
    impl Drop for HostDisplay {
        fn drop(&mut self) {
            self.display.detach();
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{Duration, Instant};
        use trames_memory::{Memory, Permissions};
        use trames_vulkan::{Backend, Invocation};
        fn invoke(backend: &mut Backend, memory: &mut Memory, operation: u64, args: &[u64]) -> u64 {
            let mut record = [0; 128];
            for (i, value) in args.iter().enumerate() {
                record[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
            memory.write(0x1000, &record).unwrap();
            let Invocation::Complete(value) =
                backend.invoke(memory, operation, 0x1000, 128).unwrap()
            else {
                panic!("unexpected pending WSI control")
            };
            value
        }
        #[test]
        #[ignore = "requires a real X11 server and Vulkan XCB ICD; validate-wsi.py provides Xvfb"]
        fn x11_events_resize_and_surface_teardown_keep_original_native_owners() {
            let display = trames_session::VulkanDisplay::default();
            let mut host = HostDisplay::open(display.clone(), 64, 48).unwrap();
            let lease = Arc::downgrade(&host.window);
            let mut backend = Backend::new().with_display(display.clone());
            let mut memory = Memory::default();
            memory.map(0x1000, 0x4000, Permissions::RW).unwrap();
            assert_eq!(invoke(&mut backend, &mut memory, 1, &[0x2000, 1]), 0);
            let instance = memory.read_u64(0x2000).unwrap();
            assert_eq!(
                invoke(&mut backend, &mut memory, 81, &[instance, 0x2000]),
                0
            );
            let surface = memory.read_u64(0x2000).unwrap();
            let mut wait = trames_linux_abi::HostWait::new().unwrap();
            wait.begin();
            host.interest(&mut wait).unwrap();
            host.window
                .native
                .configure_window(
                    host.window.id,
                    &x11rb::protocol::xproto::ConfigureWindowAux::new()
                        .width(96)
                        .height(64),
                )
                .unwrap()
                .check()
                .unwrap();
            wait.until(Instant::now() + Duration::from_secs(5));
            wait.wait().unwrap();
            host.poll().unwrap();
            assert_eq!(display.info(), (96, 64, 0));
            // Resize creates a new generation but cannot steal the live claim.
            assert_eq!(
                invoke(&mut backend, &mut memory, 81, &[instance, 0x2000]),
                (-1000000001i64) as u64
            );
            wait.begin();
            host.interest(&mut wait).unwrap();
            host.window
                .native
                .send_event(
                    false,
                    host.window.id,
                    EventMask::NO_EVENT,
                    x11rb::protocol::xproto::ClientMessageEvent::new(
                        32,
                        host.window.id,
                        host.protocols,
                        [host.close, 0, 0, 0, 0],
                    ),
                )
                .unwrap()
                .check()
                .unwrap();
            wait.until(Instant::now() + Duration::from_secs(5));
            wait.wait().unwrap();
            host.poll().unwrap();
            assert!(host.closed());
            drop(host);
            assert_eq!(display.info(), (0, 0, 0));
            assert!(lease.upgrade().is_some());
            assert_eq!(
                invoke(&mut backend, &mut memory, 82, &[instance, surface]),
                0
            );
            assert!(lease.upgrade().is_none());
            assert_eq!(invoke(&mut backend, &mut memory, 2, &[instance]), 0);
        }
    }
}
#[cfg(target_os = "linux")]
pub use x11::HostDisplay;
#[cfg(all(test, target_os = "linux"))]
mod wayland_tests {
    use std::{ffi::c_void, sync::Arc};
    use trames_memory::{Memory, Permissions};
    use trames_vulkan::{Backend, Invocation};
    type Free = unsafe extern "C" fn(*mut c_void);
    struct Fixture {
        _library: libloading::Library,
        pointer: usize,
        free: Free,
    }
    // SAFETY: the test owns original libwayland objects. Vulkan uses the
    // thread-safe display; final Arc drops them after native surface free.
    unsafe impl Send for Fixture {}
    unsafe impl Sync for Fixture {}
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe { (self.free)(self.pointer as *mut c_void) };
        }
    }
    #[test]
    #[ignore = "requires Weston/libwayland fixture; validate-wsi.py supplies both"]
    fn original_wayland_target_uses_native_surface_and_retains_its_connection() {
        let path = std::env::var_os("TRAMES_WAYLAND_FIXTURE").expect("native Wayland fixture");
        // SAFETY: acceptance builds this explicit test-only library from
        // wayland-target.c; these signatures match its C declarations.
        let (owner, connection, surface) = unsafe {
            let library = libloading::Library::new(path).unwrap();
            let create = library
                .get::<unsafe extern "C" fn() -> *mut c_void>(b"test_target_new\0")
                .unwrap();
            let get_display = library
                .get::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(b"test_target_display\0")
                .unwrap();
            let get_surface = library
                .get::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(b"test_target_surface\0")
                .unwrap();
            let free = *library.get::<Free>(b"test_target_free\0").unwrap();
            let pointer = create();
            assert!(!pointer.is_null());
            let connection = get_display(pointer);
            let surface = get_surface(pointer);
            (
                Arc::new(Fixture {
                    _library: library,
                    pointer: pointer as usize,
                    free,
                }),
                connection,
                surface,
            )
        };
        let weak = Arc::downgrade(&owner);
        let display = trames_session::VulkanDisplay::default();
        // SAFETY: Fixture retains the configured display, surface and XDG roles.
        unsafe {
            display
                .attach_wayland(connection, surface, 64, 48, owner.clone())
                .unwrap();
        }
        drop(owner);
        let mut backend = Backend::new().with_display(display.clone());
        let mut memory = Memory::default();
        memory.map(0x1000, 0x4000, Permissions::RW).unwrap();
        let invoke = |backend: &mut Backend, memory: &mut Memory, op, args: &[u64]| {
            let mut record = [0; 128];
            for (i, value) in args.iter().enumerate() {
                record[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
            memory.write(0x1000, &record).unwrap();
            assert_eq!(
                backend.invoke(memory, op, 0x1000, 128).unwrap(),
                Invocation::Complete(0)
            );
        };
        invoke(&mut backend, &mut memory, 1, &[0x2000, 1]);
        let instance = memory.read_u64(0x2000).unwrap();
        invoke(&mut backend, &mut memory, 81, &[instance, 0x2000]);
        let surface = memory.read_u64(0x2000).unwrap();
        display.detach();
        assert!(weak.upgrade().is_some());
        invoke(&mut backend, &mut memory, 82, &[instance, surface]);
        assert!(weak.upgrade().is_none());
        invoke(&mut backend, &mut memory, 2, &[instance]);
    }
}
#[cfg(not(target_os = "linux"))]
pub struct HostDisplay;
#[cfg(not(target_os = "linux"))]
impl HostDisplay {
    pub fn open(_: trames_session::VulkanDisplay, _: u32, _: u32) -> std::io::Result<Self> {
        Err(std::io::Error::other("X11 presentation requires Linux"))
    }
    pub fn closed(&self) -> bool {
        false
    }
    pub fn interest(&self, _: &mut trames_linux_abi::HostWait) -> std::io::Result<()> {
        Ok(())
    }
    pub fn poll(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
