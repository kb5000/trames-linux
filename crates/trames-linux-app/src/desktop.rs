// SPDX-License-Identifier: GPL-3.0-or-later
//! SHM desktop presentation on the existing foreground loop. Protocol state and
//! guest resource ownership stay in the shared compositor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RendererMode {
    #[default]
    Auto,
    Software,
    Vulkan,
}
#[cfg(target_os = "linux")]
impl RendererMode {
    pub fn gpu(self, scene: &trames_wayland::WindowSnapshot) -> bool {
        self == Self::Vulkan
            || (self == Self::Auto
                && scene
                    .scene
                    .iter()
                    .any(|p| matches!(p.buffer, trames_wayland::ImageBuffer::Gpu(_))))
    }
}
#[cfg(target_os = "linux")]
mod x11 {
    use std::{
        collections::{BTreeMap, BTreeSet, VecDeque},
        io,
    };
    use trames_wayland::{InputEvent, Server, WindowSnapshot};
    use x11rb::{
        connection::{Connection, RequestConnection},
        protocol::{
            xproto::{
                self, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, ImageFormat, PropMode,
                WindowClass,
            },
            Event,
        },
        wrapper::ConnectionExt as _,
        xcb_ffi::XCBConnection,
        COPY_DEPTH_FROM_PARENT,
    };

    fn error(e: impl std::fmt::Display) -> io::Error {
        io::Error::other(e.to_string())
    }
    enum Command {
        Map(std::sync::Arc<[u8]>),
        Repeat(i32, i32),
        Input(InputEvent),
        Text(trames_wayland::TextEdit),
        Configure(u64, u32, u32, trames_wayland::WindowState, u64),
        Close(u64),
        Presented(u64, u64),
    }
    type Metadata = (String, Option<u32>, (u32, u32), (u32, u32));
    struct Window {
        id: u32,
        gc: u32,
        scene: WindowSnapshot,
        size: (u32, u32),
        bounds: [i32; 4],
        dirty: bool,
        presented: Option<u64>,
        gpu: Option<crate::gpu_desktop::Renderer>,
        in_flight: Option<u64>,
        state: Option<trames_wayland::WindowState>,
        confirmed: Option<trames_wayland::WindowState>,
        dispatched: trames_wayland::WindowRequest,
        expected_size: Option<(u32, u32)>,
        interaction: Option<(u32, u32)>,
        metadata: Option<Metadata>,
    }
    pub struct Desktop {
        connection: std::sync::Arc<XCBConnection>,
        driver: XCBConnection,
        mode: super::RendererMode,
        clipboard: crate::clipboard_x11::Clipboard,
        ime: crate::ime_x11::Ime,
        keyboard: crate::keyboard_x11::Keyboard,
        wm: crate::window_x11::Manager,
        press: Option<(u64, i16, i16, u8, u32)>,
        screen: usize,
        server: Server,
        windows: BTreeMap<u64, Window>,
        protocols: u32,
        close: u32,
        revision: Option<u64>,
        commands: VecDeque<Command>,
        keys: BTreeSet<u8>,
        focus: Option<u64>,
        destroyed: BTreeSet<u64>,
        wake: Option<std::sync::Arc<trames_shared::HostWake>>,
        pending_event: Option<Event>,
        depth: u8,
        msb: bool,
    }
    impl Desktop {
        pub fn open(server: Server, mode: super::RendererMode) -> io::Result<Self> {
            x11rb::xcb_ffi::load_libxcb().map_err(error)?;
            let (connection, screen) = XCBConnection::connect(None).map_err(error)?;
            let connection = std::sync::Arc::new(connection);
            // Native WSI may read its own XCB queue; it must not consume input readiness.
            let (driver, _) = XCBConnection::connect(None).map_err(error)?;
            let setup = connection.setup();
            let root = &setup.roots[screen];
            let visual = root
                .allowed_depths
                .iter()
                .flat_map(|d| &d.visuals)
                .find(|v| v.visual_id == root.root_visual)
                .ok_or_else(|| error("missing X11 visual"))?;
            if !setup
                .pixmap_formats
                .iter()
                .any(|f| f.depth == root.root_depth && f.bits_per_pixel == 32)
                || (visual.red_mask, visual.green_mask, visual.blue_mask)
                    != (0xff0000, 0xff00, 0xff)
            {
                return Err(error("SHM desktop requires a 32-bit RGB X11 visual"));
            }
            let depth = root.root_depth;
            let msb = setup.image_byte_order == xproto::ImageOrder::MSB_FIRST;
            let protocols = connection
                .intern_atom(false, b"WM_PROTOCOLS")
                .map_err(error)?
                .reply()
                .map_err(error)?
                .atom;
            let close = connection
                .intern_atom(false, b"WM_DELETE_WINDOW")
                .map_err(error)?
                .reply()
                .map_err(error)?
                .atom;
            let keyboard = crate::keyboard_x11::Keyboard::new(&connection)?;
            let map = keyboard.map(&connection)?;
            let (rate, delay) = keyboard.repeat(&connection)?;
            let initial = VecDeque::from([Command::Map(map), Command::Repeat(rate, delay)]);
            let ime = crate::ime_x11::Ime::new(connection.clone(), screen)?;
            let clipboard = crate::clipboard_x11::Clipboard::new(&connection, screen, &server)?;
            server.output_scale(0, 1)?;
            server.manage_windows()?;
            let wm = crate::window_x11::Manager::new(&connection)?;
            Ok(Self {
                connection,
                driver,
                mode,
                clipboard,
                wm,
                press: None,
                ime,
                keyboard,
                screen,
                server,
                windows: BTreeMap::new(),
                protocols,
                close,
                revision: None,
                commands: initial,
                keys: BTreeSet::new(),
                focus: None,
                destroyed: BTreeSet::new(),
                wake: None,
                pending_event: None,
                depth,
                msb,
            })
        }
        pub fn interest(&mut self, wait: &mut trames_linux_abi::HostWait) -> io::Result<()> {
            use std::os::fd::AsFd;
            self.clipboard.interest(wait, &self.server)?;
            self.wake = Some(wait.wake());
            if self.pending_event.is_some() {
                wait.wake().notify();
            }
            wait.watch(self.connection.as_fd(), false)?;
            wait.retain([self.server.subscribe(wait.signal())]);
            if !self.commands.is_empty() {
                wait.retain([self.server.subscribe_commands(wait.signal())]);
            }
            Ok(())
        }
        fn input(&mut self, event: InputEvent) {
            self.commands.push_back(Command::Input(event));
        }
        fn focus(&mut self, window: Option<u64>) {
            if self.focus != window {
                // Shared seat focus loss releases keys/buttons, including when
                // the host intercepts a shortcut before its key-up arrives.
                self.keys.clear();
                self.focus = window;
                self.input(InputEvent::Focus { window });
                if window.is_some() {
                    self.input(InputEvent::Modifiers(self.keyboard.modifiers));
                }
            }
        }
        fn flush_commands(&mut self) -> io::Result<()> {
            while let Some(command) = self.commands.front() {
                let result = match command {
                    Command::Map(bytes) => self.server.keyboard_map(bytes.clone()),
                    Command::Repeat(r, d) => self.server.keyboard_repeat(*r, *d),
                    Command::Input(event) => self.server.input(event.clone()),
                    Command::Text(edit) => self.server.edit_text(edit.clone()),
                    Command::Configure(id, w, h, s, request) => {
                        self.server.configure_window(*id, *w, *h, *s, *request)
                    }
                    Command::Close(id) => self.server.close(*id),
                    Command::Presented(id, revision) => self.server.presented(*id, *revision, 0),
                };
                match result {
                    Ok(()) => {
                        self.commands.pop_front();
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
        fn pointer(&mut self, window: u64, x: i16, y: i16, time: u32) {
            let w = &self.windows[&window];
            let x = f64::from(w.bounds[0])
                + f64::from(x) * f64::from(w.bounds[2]) / f64::from(w.size.0);
            let y = f64::from(w.bounds[1])
                + f64::from(y) * f64::from(w.bounds[3]) / f64::from(w.size.1);
            self.input(InputEvent::Pointer { window, x, y, time });
        }
        pub fn poll(&mut self) -> io::Result<()> {
            loop {
                let forwarded = self.ime.key();
                let ime_forwarded = forwarded.is_some();
                let event = if forwarded.is_some() {
                    forwarded
                } else {
                    match self.pending_event.take() {
                        Some(event) => Some(event),
                        None => self.connection.poll_for_event().map_err(error)?,
                    }
                };
                let Some(event) = event else { break };
                if let Some(updates) = self.keyboard.event(&self.connection, &event)? {
                    for update in updates {
                        match update {
                            crate::keyboard_x11::Update::Map(m) => {
                                self.commands.push_back(Command::Map(m))
                            }
                            crate::keyboard_x11::Update::Repeat(r, d) => {
                                self.commands.push_back(Command::Repeat(r, d))
                            }
                            crate::keyboard_x11::Update::Modifiers(m) => {
                                if self.focus.is_some() {
                                    self.input(InputEvent::Modifiers(m));
                                }
                            }
                        }
                    }
                    continue;
                }
                if !ime_forwarded && self.ime.event(&event)? {
                    continue;
                }
                if self.clipboard.event(&self.connection, &event)? {
                    continue;
                }
                let native = match &event {
                    Event::PresentCompleteNotify(e) => e.window,
                    Event::Expose(e) => e.window,
                    Event::ConfigureNotify(e) => e.window,
                    Event::PropertyNotify(e) => e.window,
                    Event::MapNotify(e) => e.window,
                    Event::ClientMessage(e) => e.window,
                    Event::DestroyNotify(e) => e.window,
                    Event::FocusIn(e) | Event::FocusOut(e) => e.event,
                    Event::KeyPress(e) | Event::KeyRelease(e) => e.event,
                    Event::ButtonPress(e) | Event::ButtonRelease(e) => e.event,
                    Event::MotionNotify(e) => e.event,
                    Event::EnterNotify(e) | Event::LeaveNotify(e) => e.event,
                    Event::Error(e) => return Err(error(format!("X11 desktop: {e:?}"))),
                    _ => continue,
                };
                let Some(id) = self
                    .windows
                    .iter()
                    .find_map(|(id, w)| (w.id == native).then_some(*id))
                else {
                    continue;
                };
                match event {
                    Event::PresentCompleteNotify(e)
                        if e.kind == x11rb::protocol::present::CompleteKind::NOTIFY_MSC
                            && self.windows[&id]
                                .in_flight
                                .is_some_and(|r| r as u32 == e.serial) =>
                    {
                        let w = self.windows.get_mut(&id).unwrap();
                        if let Some(r) = w.in_flight.take() {
                            w.presented = Some(r);
                            self.commands.push_back(Command::Presented(id, r));
                        }
                    }
                    Event::Expose(_) => self.windows.get_mut(&id).unwrap().dirty = true,
                    Event::PropertyNotify(e)
                        if e.atom == self.wm.state || e.atom == self.wm.wm_state =>
                    {
                        self.confirm_window(id, native)?;
                    }
                    Event::MapNotify(_) | Event::ConfigureNotify(_) => {
                        self.confirm_window(id, native)?;
                    }
                    Event::ClientMessage(e)
                        if e.type_ == self.protocols
                            && e.format == 32
                            && e.data.as_data32()[0] == self.close =>
                    {
                        self.commands.push_back(Command::Close(id))
                    }
                    Event::DestroyNotify(_) => {
                        if self.focus == Some(id) {
                            self.focus(None);
                        }
                        if let Some(mut w) = self.windows.remove(&id) {
                            w.gpu.take();
                            self.connection.free_gc(w.gc).map_err(error)?;
                        }
                        self.destroyed.insert(id);
                        self.commands.push_back(Command::Close(id));
                    }
                    Event::FocusIn(_) => self.focus(Some(id)),
                    Event::FocusOut(e) if e.detail != xproto::NotifyDetail::INFERIOR => {
                        self.focus(None)
                    }
                    Event::EnterNotify(e) => self.pointer(id, e.event_x, e.event_y, e.time),
                    Event::MotionNotify(e) => self.pointer(id, e.event_x, e.event_y, e.time),
                    Event::ButtonPress(e) | Event::ButtonRelease(e) => {
                        let pressed = e.response_type & 0x7f == xproto::BUTTON_PRESS_EVENT;
                        self.pointer(id, e.event_x, e.event_y, e.time);
                        if pressed && matches!(e.detail, 1..=3) {
                            self.press = Some((id, e.root_x, e.root_y, e.detail, e.time));
                        }
                        if !pressed && self.press.is_some_and(|p| p.3 == e.detail) {
                            self.press = None;
                        }
                        if pressed {
                            self.focus(Some(id));
                            self.connection
                                .set_input_focus(xproto::InputFocus::PARENT, native, e.time)
                                .map_err(error)?;
                        }
                        match e.detail {
                            4..=7 if pressed => self.input(InputEvent::Scroll {
                                horizontal: match e.detail {
                                    6 => -10.,
                                    7 => 10.,
                                    _ => 0.,
                                },
                                vertical: match e.detail {
                                    4 => -10.,
                                    5 => 10.,
                                    _ => 0.,
                                },
                                time: e.time,
                            }),
                            1..=3 | 8 | 9 => self.input(InputEvent::Button {
                                button: match e.detail {
                                    1 => 272,
                                    2 => 274,
                                    3 => 273,
                                    8 => 275,
                                    _ => 276,
                                },
                                pressed,
                                time: e.time,
                            }),
                            _ => (),
                        }
                    }
                    Event::KeyPress(e) | Event::KeyRelease(e) if e.detail >= 8 => {
                        if !ime_forwarded && self.ime.forward(&e)? {
                            continue;
                        }
                        let pressed = e.response_type & 0x7f == xproto::KEY_PRESS_EVENT;
                        let changed = if pressed {
                            self.keys.insert(e.detail)
                        } else {
                            self.keys.remove(&e.detail)
                        };
                        if changed {
                            self.input(InputEvent::RawKey {
                                key: u32::from(e.detail) - 8,
                                pressed,
                                time: e.time,
                            });
                        }
                    }
                    _ => (),
                }
            }
            self.clipboard.poll(&self.connection, &self.server)?;
            self.flush_commands()?;
            let snapshot = self.server.snapshot();
            let mut text = snapshot.text.clone();
            let native = text.as_mut().and_then(|c| {
                self.windows.get(&c.window).map(|w| {
                    c.caret = [
                        ((c.caret[0] - w.bounds[0]) as i64 * w.size.0 as i64 / w.bounds[2] as i64)
                            as i32,
                        ((c.caret[1] - w.bounds[1]) as i64 * w.size.1 as i64 / w.bounds[3] as i64)
                            as i32,
                        c.caret[2],
                        c.caret[3],
                    ];
                    w.id
                })
            });
            self.ime.sync(text.as_ref(), native)?;
            self.commands.extend(self.ime.edits().map(Command::Text));
            if self.revision != Some(snapshot.revision) {
                let stale: Vec<_> = self
                    .windows
                    .keys()
                    .filter(|id| !snapshot.windows.iter().any(|s| s.id == **id))
                    .copied()
                    .collect();
                for id in stale {
                    if self.focus == Some(id) {
                        self.focus(None);
                    }
                    if let Some(mut w) = self.windows.remove(&id) {
                        w.gpu.take();
                        self.connection.free_gc(w.gc).map_err(error)?;
                        self.connection.destroy_window(w.id).map_err(error)?;
                    }
                }
                self.destroyed
                    .retain(|id| snapshot.windows.iter().any(|s| s.id == *id));
                for scene in snapshot.windows.iter().cloned() {
                    if self.destroyed.contains(&scene.id) {
                        continue;
                    }
                    if scene.scene.is_empty() {
                        continue;
                    }
                    if let Some(w) = self.windows.get_mut(&scene.id) {
                        if w.scene.revision != scene.revision {
                            let bounds = scene.bounds();
                            if bounds != w.bounds {
                                let size = (
                                    bounds[2].clamp(1, 4096) as u32,
                                    bounds[3].clamp(1, 4096) as u32,
                                );
                                if w.expected_size == Some(size) {
                                    w.expected_size = None;
                                }
                                let requested = snapshot.window_states.get(&scene.id);
                                let floating = w
                                    .state
                                    .is_some_and(|s| !s.maximized && !s.fullscreen && !s.minimized)
                                    && requested.is_none_or(|s| {
                                        !s.maximized && !s.fullscreen && !s.minimized
                                    });
                                if size != w.size && w.expected_size.is_none() && floating {
                                    self.connection
                                        .configure_window(
                                            w.id,
                                            &xproto::ConfigureWindowAux::new()
                                                .width(size.0)
                                                .height(size.1),
                                        )
                                        .map_err(error)?;
                                    w.size = size;
                                }
                            }
                            w.bounds = bounds;
                            w.dirty = true;
                            if w.scene.title != scene.title {
                                self.connection
                                    .change_property8(
                                        PropMode::REPLACE,
                                        w.id,
                                        AtomEnum::WM_NAME,
                                        AtomEnum::STRING,
                                        scene.title.as_bytes(),
                                    )
                                    .map_err(error)?;
                            }
                            w.scene = scene;
                        }
                    } else {
                        let next = snapshot
                            .window_states
                            .get(&scene.id)
                            .copied()
                            .unwrap_or_default();
                        let request = snapshot
                            .window_requests
                            .get(&scene.id)
                            .copied()
                            .unwrap_or_default();
                        self.create(scene, next, request)?;
                    }
                }
                let ids = self
                    .windows
                    .iter()
                    .map(|(&id, w)| (id, w.id))
                    .collect::<BTreeMap<_, _>>();
                let root = self.connection.setup().roots[self.screen].root;
                for scene in snapshot.windows {
                    let Some(w) = self.windows.get_mut(&scene.id) else {
                        continue;
                    };
                    let parent = scene.parent.and_then(|p| ids.get(&p).copied());
                    if w.metadata.as_ref().is_none_or(|m| {
                        m.0 != scene.title
                            || m.1 != parent
                            || m.2 != scene.min_size
                            || m.3 != scene.max_size
                    }) {
                        self.wm.metadata(&self.connection, w.id, &scene, parent)?;
                        w.metadata =
                            Some((scene.title.clone(), parent, scene.min_size, scene.max_size));
                    }
                    let next = snapshot
                        .window_states
                        .get(&scene.id)
                        .copied()
                        .unwrap_or_default();
                    let request = snapshot
                        .window_requests
                        .get(&scene.id)
                        .copied()
                        .unwrap_or_default();
                    if request != w.dispatched {
                        for field in request.changes(w.dispatched) {
                            self.wm
                                .state(&self.connection, root, w.id, w.state, next, field)?;
                        }
                        w.state = Some(next);
                        w.dispatched = request;
                    }
                    if let Some(i) = scene.interaction {
                        let gesture = (i.serial, i.edges);
                        if w.interaction != Some(gesture) {
                            if let Some((id, x, y, button, time)) =
                                self.press.filter(|p| p.0 == scene.id)
                            {
                                let _ = id;
                                self.wm.interact(
                                    &self.connection,
                                    root,
                                    w.id,
                                    i.edges,
                                    (x, y, button, time),
                                )?;
                            }
                            w.interaction = Some(gesture);
                        }
                    }
                    w.scene = scene;
                }
                self.revision = Some(snapshot.revision);
            }
            let mut acknowledgements = vec![];
            for w in self
                .windows
                .values_mut()
                .filter(|w| w.dirty && w.in_flight.is_none())
            {
                if self.mode.gpu(&w.scene) {
                    if w.gpu.as_ref().is_some_and(|g| !g.matches(&w.scene)) {
                        w.gpu.take();
                    }
                    if w.gpu.is_none() {
                        use x11rb::protocol::present::{ConnectionExt as _, EventMask};
                        let event = self.connection.generate_id().map_err(error)?;
                        self.connection
                            .present_select_input(event, w.id, EventMask::COMPLETE_NOTIFY)
                            .map_err(error)?
                            .check()
                            .map_err(error)?;
                        self.connection.flush().map_err(error)?;
                        // SAFETY: driver and native window outlive this renderer; teardown drops it first.
                        w.gpu = Some(unsafe {
                            crate::gpu_desktop::Renderer::new(
                                crate::gpu_desktop::Target::Xcb {
                                    connection: self.driver.get_raw_xcb_connection() as usize,
                                    window: w.id,
                                },
                                &w.scene,
                                w.size,
                            )
                        }?);
                    }
                    w.gpu
                        .as_mut()
                        .unwrap()
                        .present(&w.scene, w.size, w.bounds)?;
                    // A software/native ICD need not itself use XPresent. Order
                    // the driver's native requests before our own vblank notification.
                    self.driver
                        .get_input_focus()
                        .map_err(error)?
                        .reply()
                        .map_err(error)?;
                    use x11rb::protocol::present::ConnectionExt as _;
                    self.connection
                        .present_notify_msc(w.id, w.scene.revision as u32, 0, 0, 0)
                        .map_err(error)?;
                    w.in_flight = Some(w.scene.revision);
                    w.dirty = false;
                    continue;
                }
                w.gpu.take();
                let pixels = super::render(&w.scene, w.size, w.bounds, self.msb)?;
                let stride = w.size.0 as usize * 4;
                let rows = ((self.connection.maximum_request_bytes().saturating_sub(32)) / stride)
                    .clamp(1, w.size.1 as usize);
                for (chunk, bytes) in pixels.chunks(stride * rows).enumerate() {
                    self.connection
                        .put_image(
                            ImageFormat::Z_PIXMAP,
                            w.id,
                            w.gc,
                            w.size.0 as u16,
                            (bytes.len() / stride) as u16,
                            0,
                            (chunk * rows) as i16,
                            0,
                            self.depth,
                            bytes,
                        )
                        .map_err(error)?;
                }
                w.dirty = false;
                if w.presented != Some(w.scene.revision) {
                    acknowledgements.push((w.scene.id, w.scene.revision));
                    w.presented = Some(w.scene.revision);
                }
            }
            if !acknowledgements.is_empty() {
                // Server roundtrip proves the original PutImage requests were
                // consumed before frame callbacks; this is no vblank/GPU claim.
                self.connection
                    .get_input_focus()
                    .map_err(error)?
                    .reply()
                    .map_err(error)?;
                self.commands.extend(
                    acknowledgements
                        .into_iter()
                        .map(|(id, r)| Command::Presented(id, r)),
                );
            }
            self.flush_commands()?;
            self.connection.flush().map_err(error)?;
            // Reply waits can queue XCB events after the deciding event pass.
            // Retain one and latch the existing host wake; FD readiness alone
            // cannot see a user-space XCB queue. No timer/retry loop is added.
            self.pending_event = self.connection.poll_for_event().map_err(error)?;
            if self.pending_event.is_some() {
                if let Some(wake) = &self.wake {
                    wake.notify();
                }
            }
            Ok(())
        }
        fn confirm_window(&mut self, id: u64, native: u32) -> io::Result<()> {
            let next = self.wm.read(&self.connection, native)?;
            // Event payloads can predate the property reply. Read the current
            // geometry rather than pairing old ConfigureNotify sizes with it.
            let g = self
                .connection
                .get_geometry(native)
                .map_err(error)?
                .reply()
                .map_err(error)?;
            let size = (
                u32::from(g.width).clamp(1, 4096),
                u32::from(g.height).clamp(1, 4096),
            );
            let w = self.windows.get_mut(&id).unwrap();
            if w.confirmed == Some(next) && size == w.size {
                return Ok(());
            }
            if size != w.size {
                w.size = size;
                w.expected_size =
                    (w.bounds[2] != size.0 as i32 || w.bounds[3] != size.1 as i32).then_some(size);
                w.dirty = true;
            }
            w.state = Some(next);
            w.confirmed = Some(next);
            let width = (i64::from(size.0) - i64::from(w.bounds[2]) + i64::from(w.scene.width))
                .clamp(1, 4096) as u32;
            let height = (i64::from(size.1) - i64::from(w.bounds[3]) + i64::from(w.scene.height))
                .clamp(1, 4096) as u32;
            self.commands.push_back(Command::Configure(
                id,
                width,
                height,
                next,
                w.dispatched.serial,
            ));
            Ok(())
        }
        fn create(
            &mut self,
            scene: WindowSnapshot,
            next: trames_wayland::WindowState,
            request: trames_wayland::WindowRequest,
        ) -> io::Result<()> {
            let bounds = scene.bounds();
            let size = (
                bounds[2].clamp(1, 4096) as u32,
                bounds[3].clamp(1, 4096) as u32,
            );
            let id = self.connection.generate_id().map_err(error)?;
            let gc = self.connection.generate_id().map_err(error)?;
            let root = &self.connection.setup().roots[self.screen];
            self.connection
                .create_window(
                    COPY_DEPTH_FROM_PARENT,
                    id,
                    root.root,
                    0,
                    0,
                    size.0 as u16,
                    size.1 as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new()
                        .background_pixel(root.black_pixel)
                        .event_mask(
                            EventMask::EXPOSURE
                                | EventMask::STRUCTURE_NOTIFY
                                | EventMask::KEY_PRESS
                                | EventMask::KEY_RELEASE
                                | EventMask::BUTTON_PRESS
                                | EventMask::BUTTON_RELEASE
                                | EventMask::POINTER_MOTION
                                | EventMask::ENTER_WINDOW
                                | EventMask::LEAVE_WINDOW
                                | EventMask::FOCUS_CHANGE
                                | EventMask::PROPERTY_CHANGE,
                        ),
                )
                .map_err(error)?
                .check()
                .map_err(error)?;
            self.connection
                .create_gc(gc, id, &xproto::CreateGCAux::new().graphics_exposures(0))
                .map_err(error)?;
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    id,
                    self.protocols,
                    AtomEnum::ATOM,
                    &[self.close],
                )
                .map_err(error)?;
            self.connection
                .change_property8(
                    PropMode::REPLACE,
                    id,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    scene.title.as_bytes(),
                )
                .map_err(error)?;
            self.connection
                .change_property8(
                    PropMode::REPLACE,
                    id,
                    AtomEnum::WM_CLASS,
                    AtomEnum::STRING,
                    b"trames\0Trames\0",
                )
                .map_err(error)?;
            self.wm.metadata(&self.connection, id, &scene, None)?;
            self.wm
                .state(&self.connection, root.root, id, None, next, 7)?;
            self.connection.map_window(id).map_err(error)?;
            self.windows.insert(
                scene.id,
                Window {
                    id,
                    gc,
                    scene,
                    size,
                    bounds,
                    dirty: true,
                    presented: None,
                    gpu: None,
                    in_flight: None,
                    state: Some(next),
                    confirmed: None,
                    dispatched: request,
                    expected_size: None,
                    interaction: None,
                    metadata: None,
                },
            );
            Ok(())
        }
    }
    impl Drop for Desktop {
        fn drop(&mut self) {
            let _ = self.server.input(InputEvent::Focus { window: None });
            for w in self.windows.values_mut() {
                w.gpu.take();
                let _ = self.connection.free_gc(w.gc);
                let _ = self.connection.destroy_window(w.id);
            }
            let _ = self.connection.flush();
        }
    }
}
#[cfg(target_os = "linux")]
pub enum Desktop {
    X11(Box<x11::Desktop>),
    Wayland(Box<crate::desktop_wayland::Desktop>),
}
#[cfg(target_os = "linux")]
impl Desktop {
    pub fn open_mode(server: trames_wayland::Server, mode: RendererMode) -> std::io::Result<Self> {
        x11::Desktop::open(server, mode).map(|d| Self::X11(Box::new(d)))
    }
    pub fn open_wayland(
        server: trames_wayland::Server,
        mode: RendererMode,
    ) -> std::io::Result<Self> {
        crate::desktop_wayland::Desktop::open(server, mode).map(|d| Self::Wayland(Box::new(d)))
    }
    pub fn interest(&mut self, w: &mut trames_linux_abi::HostWait) -> std::io::Result<()> {
        match self {
            Self::X11(d) => d.interest(w),
            Self::Wayland(d) => d.interest(w),
        }
    }
    pub fn poll(&mut self) -> std::io::Result<()> {
        match self {
            Self::X11(d) => d.poll(),
            Self::Wayland(d) => d.poll(),
        }
    }
}

/// Software fallback over retained SHM capabilities. Raw byte reads never form
/// Rust references to guest-writable storage. Protocol-invalid concurrent guest
/// writes can change pixels, but cannot change the retained mapping or bounds.
#[cfg(target_os = "linux")]
pub(crate) fn render(
    scene: &trames_wayland::WindowSnapshot,
    size: (u32, u32),
    bounds: [i32; 4],
    msb: bool,
) -> std::io::Result<Vec<u8>> {
    let mut output = vec![0u8; size.0 as usize * size.1 as usize * 4];
    for image in &scene.scene {
        let Some(shm) = image.buffer.shm() else {
            return Err(std::io::Error::other(
                "SHM desktop cannot present GPU images; use the fixed Vulkan display",
            ));
        };
        let required = (shm.height.saturating_sub(1) as usize)
            .checked_mul(shm.stride as usize)
            .and_then(|n| n.checked_add(shm.width as usize * 4));
        if shm.width == 0
            || shm.height == 0
            || shm.stride < shm.width.saturating_mul(4)
            || required.is_none_or(|n| n > shm.mapping.len())
        {
            return Err(std::io::Error::other("invalid retained SHM image range"));
        }
        let (dw, dh) = image.destination;
        if dw == 0 || dh == 0 {
            continue;
        }
        for y in 0..size.1 {
            let ly = f64::from(bounds[1])
                + (f64::from(y) + 0.5) * f64::from(bounds[3]) / f64::from(size.1)
                - f64::from(image.y);
            if ly < 0. || ly >= f64::from(dh) {
                continue;
            }
            for x in 0..size.0 {
                let lx = f64::from(bounds[0])
                    + (f64::from(x) + 0.5) * f64::from(bounds[2]) / f64::from(size.0)
                    - f64::from(image.x);
                if lx < 0. || lx >= f64::from(dw) {
                    continue;
                }
                let mut u =
                    f64::from(image.source[0]) + lx / f64::from(dw) * f64::from(image.source[2]);
                let mut v =
                    f64::from(image.source[1]) + ly / f64::from(dh) * f64::from(image.source[3]);
                if image.transform & 4 != 0 {
                    u = 1. - u;
                }
                (u, v) = match image.transform & 3 {
                    1 => (v, 1. - u),
                    2 => (1. - u, 1. - v),
                    3 => (1. - v, u),
                    _ => (u, v),
                };
                let sx = (u * f64::from(shm.width)).clamp(0., f64::from(shm.width - 1)) as usize;
                let sy = (v * f64::from(shm.height)).clamp(0., f64::from(shm.height - 1)) as usize;
                let source = sy * shm.stride as usize + sx * 4;
                let at = (y as usize * size.0 as usize + x as usize) * 4;
                // SAFETY: retained mapping and the full stride/range proof above;
                // no shared or mutable Rust slice aliases the external writer.
                let pixel = unsafe {
                    std::ptr::read_unaligned(shm.mapping.pointer().add(source).cast::<u32>())
                }
                .to_le_bytes();
                let alpha = if shm.alpha { u32::from(pixel[3]) } else { 255 };
                for (channel, byte) in pixel.iter().take(3).enumerate() {
                    let c = if msb { 3 - channel } else { channel };
                    output[at + c] = (u32::from(*byte)
                        + (u32::from(output[at + c]) * (255 - alpha) + 127) / 255)
                        .min(255) as u8;
                }
            }
        }
    }
    Ok(output)
}
#[cfg(not(target_os = "linux"))]
pub struct Desktop;
#[cfg(not(target_os = "linux"))]
impl Desktop {
    pub fn open_wayland(_: trames_wayland::Server, _: RendererMode) -> std::io::Result<Self> {
        Err(std::io::Error::other("--desktop wayland requires Linux"))
    }
    pub fn open_mode(_: trames_wayland::Server, _: RendererMode) -> std::io::Result<Self> {
        Err(std::io::Error::other("--desktop x11 requires Linux"))
    }
    pub fn interest(&self, _: &mut trames_linux_abi::HostWait) -> std::io::Result<()> {
        Ok(())
    }
    pub fn poll(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        io::Read,
        os::fd::AsRawFd,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    #[test]
    #[ignore = "requires X11 and the original libwayland desktop fixture"]
    fn native_wayland_desktop() {
        let server = trames_wayland::Server::new().unwrap();
        let (_, peer) = server.connect(1, 0, 0).unwrap();
        let fd = peer.as_raw_fd();
        // SAFETY: this test alone owns the socket and child lifecycle.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        let mut child =
            Command::new(std::env::var_os("TRAMES_DESKTOP_CLIENT").expect("TRAMES_DESKTOP_CLIENT"))
                .env("WAYLAND_SOCKET", fd.to_string())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
        drop(peer);
        let mut output = child.stdout.take().unwrap();
        let fd = output.as_raw_fd();
        // SAFETY: live owned child pipe; flags are local to this test.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let mode = match std::env::var("TRAMES_DESKTOP_RENDERER").as_deref() {
            Ok("vulkan") => super::RendererMode::Vulkan,
            Ok("software") => super::RendererMode::Software,
            _ => super::RendererMode::Auto,
        };
        let mut desktop = if std::env::var("TRAMES_DESKTOP_HOST").as_deref() == Ok("wayland") {
            super::Desktop::open_wayland(server, mode)
        } else {
            super::Desktop::open_mode(server, mode)
        }
        .unwrap();
        let mut wait = trames_linux_abi::HostWait::new().unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop {
            wait.begin();
            desktop.interest(&mut wait).unwrap();
            wait.watch(&output, false).unwrap();
            wait.until(deadline);
            desktop.poll().unwrap();
            let mut bytes = [0; 4096];
            loop {
                match output.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        use std::io::Write;
                        std::io::stdout().write_all(&bytes[..n]).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("child stdout: {e}"),
                }
            }
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "desktop acceptance timeout");
            wait.wait().unwrap();
        }));
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result.unwrap();
    }
}
