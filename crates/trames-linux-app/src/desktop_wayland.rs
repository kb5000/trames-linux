// SPDX-License-Identifier: GPL-3.0-or-later
//! Native host Wayland on the original foreground wait. A prepared read stays
//! armed across that wait, including when native WSI shares the wl_display.
use crate::clipboard::{Bridge, Reader, Request, Writer};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    os::fd::AsFd,
    sync::Arc,
};
use trames_wayland::{InputEvent, Server, TextContext, TextEdit, WindowSnapshot, WindowState};
use wayland_client::{
    backend::ReadEventsGuard,
    delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_data_device, wl_data_device_manager,
        wl_data_offer, wl_data_source, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat,
        wl_shm, wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::{
    wp::{
        fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1 as fm, wp_fractional_scale_v1 as fs,
        },
        text_input::{
            zv1::client::{zwp_text_input_manager_v1 as m1, zwp_text_input_v1 as t1},
            zv3::client::{zwp_text_input_manager_v3 as m3, zwp_text_input_v3 as t3},
        },
        viewporter::client::{wp_viewport, wp_viewporter as vp},
    },
    xdg::{
        shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
        xdg_output::zv1::client::{zxdg_output_manager_v1 as om, zxdg_output_v1 as oo},
    },
};
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
enum Command {
    Map(Arc<[u8]>),
    Repeat(i32, i32),
    Input(InputEvent),
    Text(TextEdit),
    Configure(u64, u32, u32, WindowState, u64),
    Close(u64),
    Presented(u64, u64),
    Clipboard,
    Outputs(Vec<trames_wayland::DesktopOutput>),
    WindowOutputs(u64, Vec<u64>, u32),
}
type Metadata = (
    String,
    String,
    Option<xdg_toplevel::XdgToplevel>,
    (u32, u32),
    (u32, u32),
);
struct HostOutput {
    proxy: wl_output::WlOutput,
    xdg: Option<oo::ZxdgOutputV1>,
    data: trames_wayland::DesktopOutput,
    integer: i32,
    logical: bool,
    logical_position: bool,
    ready: bool,
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
    state: Option<WindowState>,
    dispatched: trames_wayland::WindowRequest,
    pending: Option<(i32, i32, WindowState)>,
    confirmation: Option<(u32, u32, WindowState, u64)>,
    interaction: Option<(u32, u32)>,
    metadata: Option<Metadata>,
    geometry: Option<[i32; 4]>,
    scale: crate::display_scale::Scale,
    entered: BTreeSet<u32>,
    output_state: Option<(Vec<u64>, u32)>,
    fractional: Option<fs::WpFractionalScaleV1>,
    viewport: Option<wp_viewport::WpViewport>,
    preferred: Option<u32>,
    preferred_changed: bool,
    fallback_scale: u32,
    pixel_config: Option<((u32, u32), u32)>,
}
impl Window {
    fn pixels(&self) -> (u32, u32) {
        self.scale.size(self.size)
    }
    fn pixel_configuration(&mut self) {
        if self.pixel_config == Some((self.size, self.scale.0)) {
            return;
        }
        if let Some(viewport) = &self.viewport {
            self.surface.set_buffer_scale(1);
            viewport.set_destination(self.size.0 as i32, self.size.1 as i32);
        } else {
            self.surface
                .set_buffer_scale(self.scale.0.div_ceil(120) as i32);
        }
        self.pixel_config = Some((self.size, self.scale.0));
    }
    fn native_geometry(&self) -> [i32; 4] {
        let extra = (
            self.bounds[2] - self.scene.width as i32,
            self.bounds[3] - self.scene.height as i32,
        );
        [
            -self.bounds[0],
            -self.bounds[1],
            (self.size.0 as i32 - extra.0).max(1),
            (self.size.1 as i32 - extra.1).max(1),
        ]
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        self.gpu.take();
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
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
    outputs: BTreeMap<u32, HostOutput>,
    outputs_dirty: bool,
    output_manager: Option<om::ZxdgOutputManagerV1>,
    fractional_manager: Option<fm::WpFractionalScaleManagerV1>,
    viewporter: Option<vp::WpViewporter>,
    commands: VecDeque<Command>,
    revision: Option<u64>,
    focus: Option<u64>,
    pointer_window: Option<u64>,
    press: Option<(u64, u32, u32)>,
    position: (f64, f64),
    modifiers: trames_wayland::KeyboardModifiers,
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
            outputs: BTreeMap::new(),
            outputs_dirty: false,
            output_manager: None,
            fractional_manager: None,
            viewporter: None,
            commands: VecDeque::new(),
            revision: None,
            focus: None,
            pointer_window: None,
            press: None,
            position: (0., 0.),
            modifiers: Default::default(),
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
        if let Some(manager) = &self.output_manager {
            for (&id, o) in &mut self.outputs {
                if o.xdg.is_none() {
                    o.xdg = Some(manager.get_xdg_output(&o.proxy, q, id));
                }
            }
        }
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
    fn sync_outputs(&mut self) {
        if self.outputs_dirty {
            let outputs = self
                .outputs
                .values()
                .filter(|o| o.ready)
                .map(|o| {
                    let mut data = o.data.clone();
                    let oriented = if data.transform % 2 == 1 {
                        (data.pixel_size.1, data.pixel_size.0)
                    } else {
                        data.pixel_size
                    };
                    if !o.logical {
                        data.logical_size = (
                            oriented.0 / (o.integer.max(1) as u32),
                            oriented.1 / (o.integer.max(1) as u32),
                        );
                    }
                    if data.logical_size.0 > 0 {
                        data.scale_120 = ((u64::from(oriented.0) * 120
                            + u64::from(data.logical_size.0) / 2)
                            / u64::from(data.logical_size.0))
                        .clamp(120, 480) as u32;
                    }
                    data
                })
                .collect();
            self.commands.push_back(Command::Outputs(outputs));
            self.outputs_dirty = false;
        }
        for (&id, w) in &mut self.windows {
            let members = w
                .entered
                .iter()
                .filter(|id| self.outputs.contains_key(id))
                .map(|&v| u64::from(v))
                .collect::<Vec<_>>();
            let fallback = w
                .entered
                .iter()
                .filter_map(|id| {
                    self.outputs.get(id).map(|o| {
                        let pixels = if o.data.transform % 2 == 1 {
                            o.data.pixel_size.1
                        } else {
                            o.data.pixel_size.0
                        };
                        if o.logical && o.data.logical_size.0 > 0 {
                            ((u64::from(pixels) * 120 + u64::from(o.data.logical_size.0) / 2)
                                / u64::from(o.data.logical_size.0))
                            .clamp(120, 480) as u32
                        } else {
                            o.integer.max(1) as u32 * 120
                        }
                    })
                })
                .max()
                .unwrap_or(w.scale.0);
            // Some hosts keep a stale preferred-scale hint when an output is
            // unplugged. Fresh hints win; otherwise use the current membership.
            let moved = w
                .output_state
                .as_ref()
                .is_some_and(|(old, _)| old != &members);
            let density_changed = w.fallback_scale != fallback;
            w.fallback_scale = fallback;
            if (moved || density_changed) && !w.preferred_changed {
                w.preferred = None;
            }
            let requested = w.preferred.unwrap_or(fallback);
            w.preferred_changed = false;
            let scale = if w.viewport.is_some() {
                requested
            } else {
                requested.div_ceil(120) * 120
            };
            let scale = crate::display_scale::Scale::new(scale);
            if w.output_state.as_ref() != Some(&(members.clone(), scale.0)) {
                self.commands
                    .push_back(Command::WindowOutputs(id, members.clone(), scale.0));
                w.output_state = Some((members, scale.0));
            }
            if w.scale != scale {
                w.scale = scale;
                w.dirty = true;
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
                Command::Map(m) => self.server.keyboard_map(m.clone()),
                Command::Repeat(r, d) => self.server.keyboard_repeat(*r, *d),
                Command::Input(e) => self.server.input(e.clone()),
                Command::Text(e) => self.server.edit_text(e.clone()),
                Command::Configure(id, w, h, state, request) => {
                    self.server.configure_window(*id, *w, *h, *state, *request)
                }
                Command::Close(id) => self.server.close(*id),
                Command::Presented(id, r) => self.server.presented(*id, *r, 0),
                Command::Clipboard => self.server.clipboard_changed(),
                Command::Outputs(outputs) => self.server.desktop_outputs(outputs.clone()),
                Command::WindowOutputs(id, outputs, scale) => {
                    self.server.window_outputs(*id, outputs.clone(), *scale)
                }
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
        self.sync_outputs();
        let snapshot = self.server.snapshot();
        self.context = snapshot.text;
        self.sync_text();
        if self.revision != Some(snapshot.revision) {
            self.windows
                .retain(|id, _| snapshot.windows.iter().any(|w| w.id == *id));
            for scene in snapshot.windows.iter().cloned() {
                if scene.scene.is_empty() {
                    continue;
                }
                if let Some(w) = self.windows.get_mut(&scene.id) {
                    if w.scene.revision != scene.revision {
                        let bounds = scene.bounds();
                        if bounds != w.bounds
                            && w.state
                                .is_some_and(|s| !s.maximized && !s.fullscreen && !s.resizing)
                        {
                            w.size = (
                                bounds[2].clamp(1, 4096) as u32,
                                bounds[3].clamp(1, 4096) as u32,
                            );
                        }
                        w.bounds = bounds;

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
                    let fractional = self
                        .fractional_manager
                        .as_ref()
                        .map(|m| m.get_fractional_scale(&surface, q, scene.id));
                    let viewport = self
                        .viewporter
                        .as_ref()
                        .map(|m| m.get_viewport(&surface, q, ()));
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
                            state: None,
                            dispatched: trames_wayland::WindowRequest::default(),
                            pending: None,
                            confirmation: None,
                            interaction: None,
                            metadata: None,
                            geometry: None,
                            scale: crate::display_scale::Scale::default(),
                            entered: BTreeSet::new(),
                            output_state: None,
                            fractional,
                            viewport,
                            preferred: None,
                            preferred_changed: false,
                            fallback_scale: 120,
                            pixel_config: None,
                        },
                    );
                }
            }
            let parents = self
                .windows
                .iter()
                .map(|(&id, w)| (id, w.top.clone()))
                .collect::<BTreeMap<_, _>>();
            for scene in snapshot.windows {
                let Some(w) = self.windows.get_mut(&scene.id) else {
                    continue;
                };
                let parent = scene.parent.and_then(|id| parents.get(&id));
                let mut commit = false;
                if w.metadata.as_ref().is_none_or(|m| {
                    m.0 != scene.title
                        || m.1 != scene.app_id
                        || m.2.as_ref() != parent
                        || m.3 != scene.min_size
                        || m.4 != scene.max_size
                }) {
                    commit = true;
                    w.top.set_title(scene.title.clone());
                    w.top.set_app_id(if scene.app_id.is_empty() {
                        "org.project-trames.Trames".into()
                    } else {
                        scene.app_id.clone()
                    });
                    w.top.set_parent(parent);
                    w.top.set_min_size(
                        scene.min_size.0.min(4096) as i32,
                        scene.min_size.1.min(4096) as i32,
                    );
                    let maximum = |value: u32, min: u32| {
                        if value == 0 {
                            0
                        } else {
                            value.max(min).min(4096) as i32
                        }
                    };
                    w.top.set_max_size(
                        maximum(scene.max_size.0, scene.min_size.0),
                        maximum(scene.max_size.1, scene.min_size.1),
                    );
                    w.metadata = Some((
                        scene.title.clone(),
                        scene.app_id.clone(),
                        parent.cloned(),
                        scene.min_size,
                        scene.max_size,
                    ));
                }
                if let Some(i) = scene.interaction {
                    let gesture = (i.serial, i.edges);
                    if w.interaction != Some(gesture) {
                        if let (Some(seat), Some((_, serial, button))) =
                            (&self.seat, self.press.filter(|p| p.0 == scene.id))
                        {
                            if i.edges == 0 {
                                w.top._move(seat, serial);
                            } else if let Ok(edges) = xdg_toplevel::ResizeEdge::try_from(i.edges) {
                                w.top.resize(seat, serial, edges);
                            }
                            // The host compositor owns this grab and consumes its
                            // physical release. End the guest's implicit grab too.
                            self.commands.push_back(Command::Input(InputEvent::Button {
                                button,
                                pressed: false,
                                time: 0,
                            }));
                        }
                        w.interaction = Some(gesture);
                    }
                }
                let geometry = w.native_geometry();
                if w.geometry != Some(geometry) {
                    w.xdg
                        .set_window_geometry(geometry[0], geometry[1], geometry[2], geometry[3]);
                    w.geometry = Some(geometry);
                    commit = true;
                }
                if commit && w.confirmation.is_none() {
                    w.surface.commit();
                }
                w.scene = scene;
            }
            self.revision = Some(snapshot.revision);
        }
        self.sync_outputs();
        let shm = self
            .shm
            .as_ref()
            .ok_or_else(|| error("host has no wl_shm"))?;
        for w in self
            .windows
            .values_mut()
            .filter(|w| w.configured && w.dirty && w.callback.is_none())
        {
            let pixels = w.pixels();
            if pixels.0 > 16384 || pixels.1 > 16384 {
                return Err(error("native framebuffer exceeds 16384 pixels per axis"));
            }
            w.pixel_configuration();
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
                            pixels,
                        )
                    }?);
                }
                w.callback = Some(w.surface.frame(q, w.scene.id));
                w.gpu
                    .as_mut()
                    .unwrap()
                    .present(&w.scene, pixels, w.bounds)?;
                w.presented = Some(w.scene.revision);
                w.dirty = false;
                if let Some((width, height, state, request)) = w.confirmation.take() {
                    self.commands.push_back(Command::Configure(
                        w.scene.id, width, height, state, request,
                    ));
                }
                continue;
            }
            w.gpu.take();
            if w.buffers.len() >= 3 {
                continue;
            }
            let image = crate::desktop::render(&w.scene, pixels, w.bounds, false)?;
            let file = trames_shared::SharedFile::anonymous()?;
            file.set_len(image.len())?;
            file.write_at(0, &image)?;
            let fd = file.duplicate_fd()?;
            let pool = shm.create_pool(fd.as_fd(), image.len() as i32, q, ());
            let buffer = pool.create_buffer(
                0,
                pixels.0 as i32,
                pixels.1 as i32,
                (pixels.0 * 4) as i32,
                wl_shm::Format::Xrgb8888,
                q,
                w.scene.id,
            );
            pool.destroy();
            w.surface.attach(Some(&buffer), 0, 0);
            w.surface
                .damage_buffer(0, 0, pixels.0 as i32, pixels.1 as i32);
            w.callback = Some(w.surface.frame(q, w.scene.id));
            w.surface.commit();
            w.buffers.push(buffer);
            w.presented = Some(w.scene.revision);
            w.dirty = false;
            if let Some((width, height, state, request)) = w.confirmation.take() {
                self.commands.push_back(Command::Configure(
                    w.scene.id, width, height, state, request,
                ));
            }
        }
        // Apply later guest requests only after committing the preceding native
        // configure. Weston can otherwise discard an unset before the earlier
        // maximized state has reached a surface commit. Revisit on native frame
        // readiness even when the shared scene revision has not changed.
        for (&id, w) in &mut self.windows {
            if w.confirmation.is_some() {
                continue;
            }
            let next = snapshot.window_states.get(&id).copied().unwrap_or_default();
            let request = snapshot
                .window_requests
                .get(&id)
                .copied()
                .unwrap_or_default();
            if request != w.dispatched {
                for field in request.changes(w.dispatched) {
                    match field {
                        1 => {
                            if next.maximized {
                                w.top.set_maximized();
                            } else {
                                w.top.unset_maximized();
                            }
                        }
                        2 => {
                            if next.fullscreen {
                                w.top.set_fullscreen(
                                    snapshot
                                        .fullscreen_outputs
                                        .get(&id)
                                        .and_then(|id| u32::try_from(*id).ok())
                                        .and_then(|id| self.outputs.get(&id))
                                        .map(|o| &o.proxy),
                                );
                            } else {
                                w.top.unset_fullscreen();
                            }
                        }
                        4 => {
                            if next.minimized {
                                w.top.set_minimized();
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                w.state = Some(next);
                w.dispatched = request;
            }
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
        state.sync_outputs();
        state.flush()?;
        state.server.manage_windows()?;
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
        if let wl_registry::Event::GlobalRemove { name } = &e {
            if let Some(o) = s.outputs.remove(name) {
                if let Some(x) = o.xdg {
                    x.destroy();
                }
                if o.proxy.version() >= 3 {
                    o.proxy.release();
                }
                s.outputs_dirty = true;
            }
            for w in s.windows.values_mut() {
                w.entered.remove(name);
            }
        }
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_compositor" => s.compositor = Some(r.bind(name, version.min(6), q, ())),
                "wp_viewporter" => s.viewporter = Some(r.bind(name, 1, q, ())),
                "wp_fractional_scale_manager_v1" => {
                    s.fractional_manager = Some(r.bind(name, 1, q, ()))
                }
                "zxdg_output_manager_v1" => {
                    s.output_manager = Some(r.bind(name, version.min(3), q, ()))
                }
                "wl_output" => {
                    let proxy = r.bind(name, version.min(4), q, name);
                    s.outputs.insert(
                        name,
                        HostOutput {
                            proxy,
                            xdg: None,
                            data: trames_wayland::DesktopOutput {
                                id: u64::from(name),
                                name: format!("WL-{name}"),
                                description: format!("Wayland output {name}"),
                                position: (0, 0),
                                logical_size: (0, 0),
                                pixel_size: (0, 0),
                                physical_mm: (0, 0),
                                scale_120: 120,
                                refresh_millihz: 60000,
                                transform: 0,
                            },
                            integer: 1,
                            logical: false,
                            logical_position: false,
                            ready: false,
                        },
                    );
                }
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
                if let Some((width, height, next)) = w.pending.take() {
                    let extra = (
                        w.bounds[2] - w.scene.width as i32,
                        w.bounds[3] - w.scene.height as i32,
                    );
                    if width > 0 && height > 0 {
                        w.size = (
                            (width + extra.0).clamp(1, 4096) as u32,
                            (height + extra.1).clamp(1, 4096) as u32,
                        );
                    }
                    // The guest's next buffer may arrive later (notably via
                    // Xwayland). Every host commit after this ACK must already
                    // use the host-confirmed maximized/fullscreen geometry.
                    let geometry = w.native_geometry();
                    p.set_window_geometry(geometry[0], geometry[1], geometry[2], geometry[3]);
                    w.geometry = Some(geometry);
                    w.state = Some(next);
                    w.confirmation = Some((
                        width.max(0) as u32,
                        height.max(0) as u32,
                        next,
                        w.dispatched.serial,
                    ));
                    if next.minimized {
                        let (width, height, state, request) = w.confirmation.take().unwrap();
                        s.commands
                            .push_back(Command::Configure(*id, width, height, state, request));
                    }
                    // xdg "activated" describes decorations, not seat focus.
                    // Only wl_keyboard enter/leave owns keyboard focus.
                }
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
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                if let Some(w) = s.windows.get_mut(id) {
                    let has = |state: xdg_toplevel::State| {
                        states
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|b| u32::from_ne_bytes(*b) == state as u32)
                    };
                    let active = has(xdg_toplevel::State::Activated);
                    let next = WindowState {
                        maximized: has(xdg_toplevel::State::Maximized),
                        fullscreen: has(xdg_toplevel::State::Fullscreen),
                        minimized: !active && w.state.is_some_and(|s| s.minimized),
                        resizing: has(xdg_toplevel::State::Resizing),
                    };
                    w.pending = Some((width, height, next));
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
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                use std::io::Read;
                if size == 0 || size > 1024 * 1024 {
                    s.failure = Some(error("host XKB map size is invalid"));
                    return;
                }
                let mut file = std::fs::File::from(fd);
                let mut bytes = vec![0; size as usize];
                match file.read_exact(&mut bytes) {
                    Ok(()) => s.commands.push_back(Command::Map(bytes.into())),
                    Err(e) => s.failure = Some(e),
                }
            }
            wl_keyboard::Event::Keymap { .. } => {
                s.failure = Some(error("host keyboard requires an XKB-v1 keymap"));
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                s.commands.push_back(Command::Repeat(rate, delay))
            }
            wl_keyboard::Event::Enter {
                serial, surface, ..
            } => {
                s.serial = serial;
                s.focus = surface.data::<u64>().copied();
                if s.text3.is_none() {
                    s.text_enter = s.focus;
                }
                s.input(InputEvent::Focus { window: s.focus });
                s.input(InputEvent::Modifiers(s.modifiers));
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
                s.input(InputEvent::RawKey {
                    key,
                    pressed: state == wl_keyboard::KeyState::Pressed,
                    time,
                });
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                s.modifiers = trames_wayland::KeyboardModifiers {
                    depressed: mods_depressed,
                    latched: mods_latched,
                    locked: mods_locked,
                    group,
                };
                if s.focus.is_some() {
                    s.input(InputEvent::Modifiers(s.modifiers));
                }
            }
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
                if state == wl_pointer::ButtonState::Pressed {
                    s.press = s.pointer_window.map(|id| (id, serial, button));
                } else if s.press.is_some_and(|p| p.2 == button) {
                    s.press = None;
                }
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
        s: &mut Self,
        _: &wl_surface::WlSurface,
        e: wl_surface::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(w) = s.windows.get_mut(id) {
            match e {
                wl_surface::Event::Enter { output } => {
                    if let Some(id) = output.data::<u32>() {
                        w.entered.insert(*id);
                    }
                }
                wl_surface::Event::Leave { output } => {
                    if let Some(id) = output.data::<u32>() {
                        w.entered.remove(id);
                    }
                }
                wl_surface::Event::PreferredBufferScale { factor } if w.fractional.is_none() => {
                    w.preferred = Some(factor.max(1) as u32 * 120);
                    w.preferred_changed = true;
                }
                _ => (),
            }
        }
    }
}
impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        s: &mut Self,
        _: &wl_output::WlOutput,
        e: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(o) = s.outputs.get_mut(id) {
            match e {
                wl_output::Event::Geometry {
                    x,
                    y,
                    physical_width,
                    physical_height,
                    transform: WEnum::Value(t),
                    ..
                } => {
                    if !o.logical_position {
                        o.data.position = (x, y);
                    }
                    o.data.physical_mm = (physical_width, physical_height);
                    o.data.transform = t as u32;
                }
                wl_output::Event::Mode {
                    flags: WEnum::Value(flags),
                    width,
                    height,
                    refresh,
                } if flags.contains(wl_output::Mode::Current) => {
                    o.data.pixel_size = (width.max(1) as u32, height.max(1) as u32);
                    o.data.refresh_millihz = refresh;
                }
                wl_output::Event::Scale { factor } => o.integer = factor.clamp(1, 4),
                wl_output::Event::Name { name } => o.data.name = name,
                wl_output::Event::Description { description } => o.data.description = description,
                wl_output::Event::Done => {
                    o.ready = true;
                    s.outputs_dirty = true;
                }
                _ => (),
            }
            if o.proxy.version() < 2 && o.data.pixel_size.0 > 0 {
                o.ready = true;
                s.outputs_dirty = true;
            }
        }
    }
}
impl Dispatch<oo::ZxdgOutputV1, u32> for State {
    fn event(
        s: &mut Self,
        _: &oo::ZxdgOutputV1,
        e: oo::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(o) = s.outputs.get_mut(id) {
            match e {
                oo::Event::LogicalPosition { x, y } => {
                    o.data.position = (x, y);
                    o.logical_position = true;
                }
                oo::Event::LogicalSize { width, height } => {
                    o.data.logical_size = (width.max(1) as u32, height.max(1) as u32);
                    o.logical = true;
                }
                oo::Event::Name { name } => o.data.name = name,
                oo::Event::Description { description } => o.data.description = description,
                oo::Event::Done => {
                    o.ready = true;
                    s.outputs_dirty = true;
                }
                _ => (),
            }
        }
    }
}
impl Dispatch<fs::WpFractionalScaleV1, u64> for State {
    fn event(
        s: &mut Self,
        _: &fs::WpFractionalScaleV1,
        e: fs::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let fs::Event::PreferredScale { scale } = e {
            if let Some(w) = s.windows.get_mut(id) {
                w.preferred = Some(scale);
                w.preferred_changed = true;
            }
        }
    }
}
delegate_noop!(State:ignore vp::WpViewporter);
delegate_noop!(State:ignore wp_viewport::WpViewport);
delegate_noop!(State:ignore fm::WpFractionalScaleManagerV1);
delegate_noop!(State:ignore om::ZxdgOutputManagerV1);
delegate_noop!(State:ignore wl_shm::WlShm);
delegate_noop!(State:ignore wl_shm_pool::WlShmPool);
delegate_noop!(State:ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(State:ignore m3::ZwpTextInputManagerV3);
delegate_noop!(State:ignore m1::ZwpTextInputManagerV1);
