// SPDX-License-Identifier: GPL-3.0-or-later
//! Native host Wayland on the original foreground wait. A prepared read stays
//! armed across that wait, including when native WSI shares the wl_display.
use crate::clipboard::{Bridge, Reader, Request, Writer};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    os::fd::AsFd,
    sync::Arc,
};
use trames_wayland::{InputEvent, Server, TextContext, TextEdit, WindowSnapshot};
use wayland_client::{
    backend::ReadEventsGuard,
    delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_data_device, wl_data_device_manager,
        wl_data_offer, wl_data_source, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::{
    wp::text_input::{
        zv1::client::{zwp_text_input_manager_v1 as m1, zwp_text_input_v1 as t1},
        zv3::client::{zwp_text_input_manager_v3 as m3, zwp_text_input_v3 as t3},
    },
    xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
};
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
enum Command {
    Input(InputEvent),
    Text(TextEdit),
    Resize(u64, u32, u32),
    Close(u64),
    Presented(u64, u64),
    Clipboard,
}
struct Window {
    gpu: Option<crate::gpu_desktop::Renderer>,
    surface: wl_surface::WlSurface,
    xdg: xdg_surface::XdgSurface,
    top: xdg_toplevel::XdgToplevel,
    scene: WindowSnapshot,
    size: (u32, u32),
    bounds: [i32; 4],
    configured: bool,
    dirty: bool,
    callback: Option<wl_callback::WlCallback>,
    buffers: Vec<wl_buffer::WlBuffer>,
    presented: Option<u64>,
}
impl Drop for Window {
    fn drop(&mut self) {
        self.gpu.take();
        self.callback.take();
        for b in self.buffers.drain(..) {
            b.destroy();
        }
        self.top.destroy();
        self.xdg.destroy();
        self.surface.destroy();
    }
}
struct State {
    mode: crate::desktop::RendererMode,
    display: usize,
    server: Server,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    windows: BTreeMap<u64, Window>,
    commands: VecDeque<Command>,
    revision: Option<u64>,
    focus: Option<u64>,
    pointer_window: Option<u64>,
    position: (f64, f64),
    modifiers: u32,
    serial: u32,
    bridge: Arc<Bridge>,
    data_manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    data: Option<wl_data_device::WlDataDevice>,
    offer: Option<wl_data_offer::WlDataOffer>,
    mimes: BTreeMap<u32, Vec<String>>,
    source: Option<wl_data_source::WlDataSource>,
    owned: Option<String>,
    marker: String,
    readers: Vec<Reader>,
    incoming: Vec<(Reader, trames_shared::NativePipe)>,
    writers: Vec<Writer>,
    export_pending: bool,
    manager3: Option<m3::ZwpTextInputManagerV3>,
    text3: Option<t3::ZwpTextInputV3>,
    manager1: Option<m1::ZwpTextInputManagerV1>,
    text1: Option<t1::ZwpTextInputV1>,
    text_enter: Option<u64>,
    context: Option<TextContext>,
    text_signature: Option<(u64, u64, u32)>,
    host_serial: u32,
    text_stamp: Option<TextContext>,
    pending_edit: TextEdit,
    preedit_cursor: i32,
    failure: Option<io::Error>,
    write_blocked: bool,
}
impl State {
    fn new(server: Server, mode: crate::desktop::RendererMode, display: usize) -> io::Result<Self> {
        let bridge = Arc::new(Bridge::default());
        server.attach_clipboard(bridge.clone())?;
        Ok(Self {
            server,
            mode,
            display,
            compositor: None,
            shm: None,
            shell: None,
            seat: None,
            keyboard: None,
            pointer: None,
            windows: BTreeMap::new(),
            commands: VecDeque::new(),
            revision: None,
            focus: None,
            pointer_window: None,
            position: (0., 0.),
            modifiers: 0,
            serial: 0,
            bridge,
            data_manager: None,
            data: None,
            offer: None,
            mimes: BTreeMap::new(),
            source: None,
            owned: None,
            marker: format!("application/x-trames-clipboard-{}", std::process::id()),
            readers: vec![],
            incoming: vec![],
            writers: vec![],
            export_pending: false,
            manager3: None,
            text3: None,
            manager1: None,
            text1: None,
            text_enter: None,
            context: None,
            text_signature: None,
            host_serial: 0,
            text_stamp: None,
            pending_edit: TextEdit::default(),
            preedit_cursor: 0,
            failure: None,
            write_blocked: false,
        })
    }
    fn services(&mut self, q: &QueueHandle<Self>) {
        if let Some(seat) = &self.seat {
            if self.data.is_none() {
                if let Some(m) = &self.data_manager {
                    self.data = Some(m.get_data_device(seat, q, ()));
                }
            }
            if self.text3.is_none() {
                if let Some(m) = &self.manager3 {
                    self.text3 = Some(m.get_text_input(seat, q, ()));
                }
            }
            if self.text1.is_none() {
                if let Some(m) = &self.manager1 {
                    self.text1 = Some(m.create_text_input(q, ()));
                }
            }
        }
    }
    fn input(&mut self, e: InputEvent) {
        self.commands.push_back(Command::Input(e));
    }
    fn pointer(&mut self, time: u32) {
        if let Some(window) = self.pointer_window {
            if let Some(w) = self.windows.get(&window) {
                let x = f64::from(w.bounds[0])
                    + self.position.0 * f64::from(w.bounds[2]) / f64::from(w.size.0);
                let y = f64::from(w.bounds[1])
                    + self.position.1 * f64::from(w.bounds[3]) / f64::from(w.size.1);
                self.input(InputEvent::Pointer { window, x, y, time });
            }
        }
    }
    fn edit(&mut self, serial: u32) {
        if serial != self.host_serial {
            return;
        }
        let Some(stamp) = self.text_stamp.as_ref() else {
            return;
        };
        if !self
            .context
            .as_ref()
            .is_some_and(|c| c.id == stamp.id && c.epoch == stamp.epoch)
        {
            self.pending_edit = TextEdit::default();
            return;
        }
        let mut edit = std::mem::take(&mut self.pending_edit);
        edit.serial = stamp.serial;
        edit.context = stamp.id;
        edit.epoch = stamp.epoch;
        self.commands.push_back(Command::Text(edit));
    }
    fn sync_text(&mut self) {
        let context = self
            .context
            .as_ref()
            .filter(|c| Some(c.window) == self.text_enter && self.focus == Some(c.window));
        let signature = context.map(|c| (c.id, c.epoch, c.serial));
        if signature == self.text_signature {
            return;
        }
        let old = self.text_signature.take();
        self.text_stamp = context.cloned();
        self.pending_edit = TextEdit::default();
        if let Some(text) = &self.text3 {
            if context.is_none() {
                text.disable();
            } else if let Some(c) = context {
                if old.map(|s| (s.0, s.1)) != Some((c.id, c.epoch)) {
                    text.enable();
                }
                text.set_surrounding_text(c.text.clone(), c.cursor as i32, c.anchor as i32);
                text.set_text_change_cause(t3::ChangeCause::Other);
                text.set_content_type(
                    t3::ContentHint::from_bits_truncate(c.hint),
                    t3::ContentPurpose::try_from(c.purpose).unwrap_or(t3::ContentPurpose::Normal),
                );
                let b = self
                    .windows
                    .get(&c.window)
                    .map(|w| w.bounds)
                    .unwrap_or([0, 0, 1, 1]);
                text.set_cursor_rectangle(
                    c.caret[0] - b[0],
                    c.caret[1] - b[1],
                    c.caret[2].max(1),
                    c.caret[3].max(1),
                );
            }
            text.commit();
            self.host_serial = self.host_serial.wrapping_add(1);
        } else if let (Some(text), Some(seat)) = (&self.text1, &self.seat) {
            if let Some(c) = context {
                if let Some(w) = self.windows.get(&c.window) {
                    if old.map(|s| (s.0, s.1)) != Some((c.id, c.epoch)) {
                        text.activate(seat, &w.surface);
                    }
                    text.set_surrounding_text(c.text.clone(), c.cursor, c.anchor);
                    text.set_content_type(
                        t1::ContentHint::from_bits_truncate(c.hint),
                        t1::ContentPurpose::try_from(c.purpose)
                            .unwrap_or(t1::ContentPurpose::Normal),
                    );
                    text.set_cursor_rectangle(
                        c.caret[0] - w.bounds[0],
                        c.caret[1] - w.bounds[1],
                        c.caret[2].max(1),
                        c.caret[3].max(1),
                    );
                    self.host_serial = self.host_serial.wrapping_add(1);
                    text.commit_state(self.host_serial);
                }
            } else {
                text.deactivate(seat);
            }
        }
        self.text_signature = signature;
    }
    fn clipboard(&mut self, q: &QueueHandle<Self>) -> io::Result<()> {
        for request in self.bridge.take() {
            if self.readers.len() + self.incoming.len() + self.writers.len() >= 16 {
                continue;
            }
            match request {
                Request::Write(p) => self.readers.push(Reader::new(p)),
                Request::Read(p) => {
                    if let Some(offer) = &self.offer {
                        let m = self.mimes.get(&offer.id().protocol_id());
                        if let Some(mime) = m.and_then(|m| {
                            ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"]
                                .into_iter()
                                .find(|x| m.iter().any(|s| s == x))
                        }) {
                            let (r, w) = trames_shared::NativePipe::pair()?;
                            offer.receive(mime.into(), w.as_fd());
                            self.incoming.push((Reader::new(r), p));
                        }
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
                    self.export_pending = true;
                    self.readers.swap_remove(n);
                }
                Err(_) => {
                    self.readers.swap_remove(n);
                }
            }
        }
        let mut n = 0;
        while n < self.incoming.len() {
            match self.incoming[n].0.poll() {
                Ok(None) => n += 1,
                Ok(Some(text)) => {
                    let (_, p) = self.incoming.swap_remove(n);
                    self.writers.push(Writer::new(p, &text));
                }
                Err(_) => {
                    self.incoming.swap_remove(n);
                }
            }
        }
        self.writers.retain_mut(|w| matches!(w.poll(), Ok(false)));
        if self.export_pending && self.serial != 0 {
            if let (Some(m), Some(d)) = (&self.data_manager, &self.data) {
                let source = m.create_data_source(q, ());
                for mime in ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"] {
                    source.offer(mime.into());
                }
                source.offer(self.marker.clone());
                d.set_selection(Some(&source), self.serial);
                if let Some(old) = self.source.replace(source) {
                    old.destroy();
                }
                self.export_pending = false;
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        while let Some(c) = self.commands.front() {
            let r = match c {
                Command::Input(e) => self.server.input(e.clone()),
                Command::Text(e) => self.server.edit_text(e.clone()),
                Command::Resize(id, w, h) => self.server.resize(*id, *w, *h),
                Command::Close(id) => self.server.close(*id),
                Command::Presented(id, r) => self.server.presented(*id, *r, 0),
                Command::Clipboard => self.server.clipboard_changed(),
            };
            match r {
                Ok(()) => {
                    self.commands.pop_front();
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn refresh(&mut self, q: &QueueHandle<Self>) -> io::Result<()> {
        let snapshot = self.server.snapshot();
        self.context = snapshot.text;
        self.sync_text();
        if self.revision != Some(snapshot.revision) {
            self.windows
                .retain(|id, _| snapshot.windows.iter().any(|w| w.id == *id));
            for scene in snapshot.windows {
                if scene.scene.is_empty() {
                    continue;
                }
                if let Some(w) = self.windows.get_mut(&scene.id) {
                    if w.scene.revision != scene.revision {
                        w.bounds = scene.bounds();
                        w.top.set_title(scene.title.clone());
                        w.scene = scene;
                        w.dirty = true;
                    }
                } else {
                    let compositor = self
                        .compositor
                        .as_ref()
                        .ok_or_else(|| error("host has no wl_compositor"))?;
                    let shell = self
                        .shell
                        .as_ref()
                        .ok_or_else(|| error("host has no xdg_wm_base"))?;
                    let surface = compositor.create_surface(q, scene.id);
                    let xdg = shell.get_xdg_surface(&surface, q, scene.id);
                    let top = xdg.get_toplevel(q, scene.id);
                    top.set_title(scene.title.clone());
                    top.set_app_id("org.project-trames.Trames".into());
                    if let Some(parent) = scene.parent.and_then(|id| self.windows.get(&id)) {
                        top.set_parent(Some(&parent.top));
                    }
                    let bounds = scene.bounds();
                    let size = (
                        bounds[2].clamp(1, 4096) as u32,
                        bounds[3].clamp(1, 4096) as u32,
                    );
                    surface.commit();
                    self.windows.insert(
                        scene.id,
                        Window {
                            gpu: None,
                            surface,
                            xdg,
                            top,
                            scene,
                            size,
                            bounds,
                            configured: false,
                            dirty: true,
                            callback: None,
                            buffers: vec![],
                            presented: None,
                        },
                    );
                }
            }
            self.revision = Some(snapshot.revision);
        }
        let shm = self
            .shm
            .as_ref()
            .ok_or_else(|| error("host has no wl_shm"))?;
        for w in self
            .windows
            .values_mut()
            .filter(|w| w.configured && w.dirty && w.callback.is_none())
        {
            if self.mode.gpu(&w.scene) {
                if w.gpu.as_ref().is_some_and(|g| !g.matches(&w.scene)) {
                    w.gpu.take();
                }
                if w.gpu.is_none() {
                    // SAFETY: refresh runs after cancelling the prepared read; Window drops WSI before surface.
                    w.gpu = Some(unsafe {
                        crate::gpu_desktop::Renderer::new(
                            crate::gpu_desktop::Target::Wayland {
                                display: self.display,
                                surface: w.surface.id().as_ptr() as usize,
                            },
                            &w.scene,
                            w.size,
                        )
                    }?);
                }
                w.callback = Some(w.surface.frame(q, w.scene.id));
                w.gpu
                    .as_mut()
                    .unwrap()
                    .present(&w.scene, w.size, w.bounds)?;
                w.presented = Some(w.scene.revision);
                w.dirty = false;
                continue;
            }
            w.gpu.take();
            if w.buffers.len() >= 3 {
                continue;
            }
            let pixels = crate::desktop::render(&w.scene, w.size, w.bounds, false)?;
            let file = trames_shared::SharedFile::anonymous()?;
            file.set_len(pixels.len())?;
            file.write_at(0, &pixels)?;
            let fd = file.duplicate_fd()?;
            let pool = shm.create_pool(fd.as_fd(), pixels.len() as i32, q, ());
            let buffer = pool.create_buffer(
                0,
                w.size.0 as i32,
                w.size.1 as i32,
                (w.size.0 * 4) as i32,
                wl_shm::Format::Xrgb8888,
                q,
                w.scene.id,
            );
            pool.destroy();
            w.surface.attach(Some(&buffer), 0, 0);
            w.surface
                .damage_buffer(0, 0, w.size.0 as i32, w.size.1 as i32);
            w.callback = Some(w.surface.frame(q, w.scene.id));
            w.surface.commit();
            w.buffers.push(buffer);
            w.presented = Some(w.scene.revision);
            w.dirty = false;
        }
        Ok(())
    }
}
pub struct Desktop {
    wake: Option<Arc<trames_shared::HostWake>>,
    guard: Option<ReadEventsGuard>,
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}
impl Desktop {
    pub fn open(server: Server, mode: crate::desktop::RendererMode) -> io::Result<Self> {
        let connection = Connection::connect_to_env().map_err(error)?;
        let mut queue = connection.new_event_queue();
        let q = queue.handle();
        let mut state = State::new(server, mode, connection.backend().display_ptr() as usize)?;
        connection.display().get_registry(&q, ());
        queue.roundtrip(&mut state).map_err(error)?;
        state.services(&q);
        queue.roundtrip(&mut state).map_err(error)?;
        if state.compositor.is_none() || state.shm.is_none() || state.shell.is_none() {
            return Err(error("Wayland host requires compositor, SHM and xdg-shell"));
        }
        state.server.output_scale(0, 1)?;
        Ok(Self {
            wake: None,
            guard: None,
            connection,
            queue,
            state,
        })
    }
    pub fn interest(&mut self, w: &mut trames_shared::HostWait) -> io::Result<()> {
        self.wake = Some(w.wake());
        w.watch(&self.connection, false)?;
        if self.state.write_blocked {
            w.watch(&self.connection, true)?;
        }
        w.retain([self.state.server.subscribe(w.signal())]);
        if !self.state.commands.is_empty() {
            w.retain([self.state.server.subscribe_commands(w.signal())]);
        }
        self.state.bridge.arm(w);
        for p in &self.state.readers {
            p.interest(w)?;
        }
        for (p, _) in &self.state.incoming {
            p.interest(w)?;
        }
        for p in &self.state.writers {
            p.interest(w)?;
        }
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<()> {
        if let Some(guard) = self.guard.take() {
            use std::os::fd::AsRawFd;
            let mut fd = libc::pollfd {
                fd: self.connection.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut fd, 1, 0) };
            if ready < 0 {
                return Err(io::Error::last_os_error());
            }
            if ready > 0 {
                match guard.read() {
                    Ok(_) => (),
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(error(e)),
                }
            }
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(error)?;
        let q = self.queue.handle();
        self.state.clipboard(&q)?;
        self.state.flush()?;
        self.state.refresh(&q)?;
        self.state.flush()?;
        if let Some(e) = self.state.failure.take() {
            return Err(e);
        }
        match self.connection.flush() {
            Ok(()) => self.state.write_blocked = false,
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock =>
            {
                self.state.write_blocked = true
            }
            Err(e) => return Err(error(e)),
        }
        loop {
            if let Some(guard) = self.connection.prepare_read() {
                self.guard = Some(guard);
                break;
            }
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(error)?;
            // Native WSI can enqueue UI events during its own dispatch. Latch
            // their resulting work before sleeping with an empty socket.
            if let Some(wake) = &self.wake {
                wake.notify();
            }
        }
        Ok(())
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.guard.take();
        let _ = self.state.server.input(InputEvent::Focus { window: None });
        self.state.windows.clear();
        let _ = self.connection.flush();
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        s: &mut Self,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_compositor" => s.compositor = Some(r.bind(name, version.min(4), q, ())),
                "wl_shm" => s.shm = Some(r.bind(name, 1, q, ())),
                "xdg_wm_base" => s.shell = Some(r.bind(name, version.min(3), q, ())),
                "wl_seat" => s.seat = Some(r.bind(name, version.min(7), q, ())),
                "wl_data_device_manager" => {
                    s.data_manager = Some(r.bind(name, version.min(3), q, ()))
                }
                "zwp_text_input_manager_v3" => s.manager3 = Some(r.bind(name, 1, q, ())),
                "zwp_text_input_manager_v1" => s.manager1 = Some(r.bind(name, 1, q, ())),
                _ => (),
            }
            s.services(q);
        }
    }
}
impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        p: &xdg_wm_base::XdgWmBase,
        e: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = e {
            p.pong(serial);
        }
    }
}
impl Dispatch<xdg_surface::XdgSurface, u64> for State {
    fn event(
        s: &mut Self,
        p: &xdg_surface::XdgSurface,
        e: xdg_surface::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = e {
            p.ack_configure(serial);
            if let Some(w) = s.windows.get_mut(id) {
                w.configured = true;
                w.dirty = true;
            }
        }
    }
}
impl Dispatch<xdg_toplevel::XdgToplevel, u64> for State {
    fn event(
        s: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        e: xdg_toplevel::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            xdg_toplevel::Event::Close => s.commands.push_back(Command::Close(*id)),
            xdg_toplevel::Event::Configure { width, height, .. } if width > 0 && height > 0 => {
                if let Some(w) = s.windows.get_mut(id) {
                    w.size = (width.min(4096) as u32, height.min(4096) as u32);
                    w.dirty = true;
                    let width = (width - w.bounds[2] + w.scene.width as i32).clamp(1, 4096) as u32;
                    let height =
                        (height - w.bounds[3] + w.scene.height as i32).clamp(1, 4096) as u32;
                    s.commands.push_back(Command::Resize(*id, width, height));
                }
            }
            _ => (),
        }
    }
}
impl Dispatch<wl_callback::WlCallback, u64> for State {
    fn event(
        s: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(w) = s.windows.get_mut(id) {
            w.callback = None;
            if let Some(r) = w.presented.take() {
                s.commands.push_back(Command::Presented(*id, r));
            }
        }
    }
}
impl Dispatch<wl_buffer::WlBuffer, u64> for State {
    fn event(
        s: &mut Self,
        p: &wl_buffer::WlBuffer,
        e: wl_buffer::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = e {
            if let Some(w) = s.windows.get_mut(id) {
                w.buffers.retain(|b| b != p);
            }
            p.destroy();
        }
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        s: &mut Self,
        p: &wl_seat::WlSeat,
        e: wl_seat::Event,
        _: &(),
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(c),
        } = e
        {
            if c.contains(wl_seat::Capability::Keyboard) && s.keyboard.is_none() {
                s.keyboard = Some(p.get_keyboard(q, ()));
            }
            if c.contains(wl_seat::Capability::Pointer) && s.pointer.is_none() {
                s.pointer = Some(p.get_pointer(q, ()));
            }
        }
    }
}
impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        e: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_keyboard::Event::Enter {
                serial, surface, ..
            } => {
                s.serial = serial;
                s.focus = surface.data::<u64>().copied();
                if s.text3.is_none() {
                    s.text_enter = s.focus;
                }
                s.input(InputEvent::Focus { window: s.focus });
                s.text_signature = None;
                s.sync_text();
            }
            wl_keyboard::Event::Leave { .. } => {
                s.focus = None;
                if s.text3.is_none() {
                    s.text_enter = None;
                }
                s.input(InputEvent::Focus { window: None });
                s.text_signature = None;
                s.sync_text();
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                key,
                state: WEnum::Value(state),
            } => {
                s.serial = serial;
                s.input(InputEvent::Key {
                    key,
                    pressed: state == wl_keyboard::KeyState::Pressed,
                    modifiers: s.modifiers,
                    time,
                });
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                ..
            } => s.modifiers = mods_depressed | mods_latched | mods_locked,
            _ => (),
        }
    }
}
impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_pointer::WlPointer,
        e: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                s.serial = serial;
                s.pointer_window = surface.data::<u64>().copied();
                s.position = (surface_x, surface_y);
                s.pointer(0);
            }
            wl_pointer::Event::Leave { .. } => s.pointer_window = None,
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => {
                s.position = (surface_x, surface_y);
                s.pointer(time);
            }
            wl_pointer::Event::Button {
                serial,
                time,
                button,
                state: WEnum::Value(state),
            } => {
                s.serial = serial;
                s.pointer(time);
                s.input(InputEvent::Button {
                    button,
                    pressed: state == wl_pointer::ButtonState::Pressed,
                    time,
                });
            }
            wl_pointer::Event::Axis {
                time,
                axis: WEnum::Value(axis),
                value,
            } => s.input(InputEvent::Scroll {
                horizontal: if axis == wl_pointer::Axis::HorizontalScroll {
                    value
                } else {
                    0.
                },
                vertical: if axis == wl_pointer::Axis::VerticalScroll {
                    value
                } else {
                    0.
                },
                time,
            }),
            _ => (),
        }
    }
}
impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_data_device::WlDataDevice,
        e: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_device::Event::Selection { id } = e {
            if let Some(old) = s.offer.take() {
                old.destroy();
            }
            let own = id
                .as_ref()
                .and_then(|p| s.mimes.get(&p.id().protocol_id()))
                .is_some_and(|m| m.contains(&s.marker));
            s.offer = id;
            s.mimes.retain(|id, _| {
                s.offer
                    .as_ref()
                    .is_some_and(|p| p.id().protocol_id() == *id)
            });
            if !own {
                s.commands.push_back(Command::Clipboard);
            }
        }
    }
    wayland_client::event_created_child!(State,wl_data_device::WlDataDevice,[0=>(wl_data_offer::WlDataOffer,())]);
}
impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn event(
        s: &mut Self,
        p: &wl_data_offer::WlDataOffer,
        e: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = e {
            s.mimes
                .entry(p.id().protocol_id())
                .or_default()
                .push(mime_type);
        }
    }
}
impl Dispatch<wl_data_source::WlDataSource, ()> for State {
    fn event(
        s: &mut Self,
        p: &wl_data_source::WlDataSource,
        e: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_data_source::Event::Send { mime_type, fd } => {
                if ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"]
                    .contains(&mime_type.as_str())
                {
                    if let Some(text) = &s.owned {
                        if let Ok(pipe) = trames_shared::NativePipe::import(fd) {
                            if s.writers.len() >= 16 {
                                return;
                            }
                            s.writers.push(Writer::new(pipe, text));
                        }
                    }
                }
            }
            wl_data_source::Event::Cancelled => {
                if s.source.as_ref() == Some(p) {
                    s.source = None;
                }
                p.destroy();
            }
            _ => (),
        }
    }
}
impl Dispatch<t3::ZwpTextInputV3, ()> for State {
    fn event(
        s: &mut Self,
        _: &t3::ZwpTextInputV3,
        e: t3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            t3::Event::Enter { surface } => {
                s.text_enter = surface.data::<u64>().copied();
                s.text_signature = None;
                s.sync_text();
            }
            t3::Event::Leave { .. } => {
                s.text_enter = None;
                s.text_signature = None;
                s.sync_text();
            }
            t3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                s.pending_edit.preedit = Some((text.unwrap_or_default(), cursor_begin, cursor_end))
            }
            t3::Event::CommitString { text } => s.pending_edit.commit = text,
            t3::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => {
                s.pending_edit.delete_before = before_length;
                s.pending_edit.delete_after = after_length;
            }
            t3::Event::Done { serial } => s.edit(serial),
            _ => (),
        }
    }
}
impl Dispatch<t1::ZwpTextInputV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &t1::ZwpTextInputV1,
        e: t1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            t1::Event::PreeditCursor { index } => s.preedit_cursor = index,
            t1::Event::PreeditString { serial, text, .. } => {
                s.pending_edit.preedit = Some((text, s.preedit_cursor, s.preedit_cursor));
                s.edit(serial);
            }
            t1::Event::CommitString { serial, text } => {
                s.pending_edit.commit = Some(text);
                s.edit(serial);
            }
            t1::Event::DeleteSurroundingText { index, length }
                if index <= 0 && i64::from(index) + i64::from(length) >= 0 =>
            {
                s.pending_edit.delete_before = index.unsigned_abs();
                s.pending_edit.delete_after = (i64::from(index) + i64::from(length)) as u32;
            }
            _ => (),
        }
    }
}
delegate_noop!(State:ignore wl_compositor::WlCompositor);
impl Dispatch<wl_surface::WlSurface, u64> for State {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State:ignore wl_shm::WlShm);
delegate_noop!(State:ignore wl_shm_pool::WlShmPool);
delegate_noop!(State:ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(State:ignore m3::ZwpTextInputManagerV3);
delegate_noop!(State:ignore m1::ZwpTextInputManagerV1);
