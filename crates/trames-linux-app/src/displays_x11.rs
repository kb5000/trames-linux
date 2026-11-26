// SPDX-License-Identifier: GPL-3.0-or-later
//! RandR topology is queried at setup/change notifications, never in CPU execution.
use crate::display_scale::Scale;
use std::{collections::BTreeMap, io};
use trames_wayland::DesktopOutput;
use x11rb::{
    connection::Connection,
    protocol::{
        randr, xinerama,
        xproto::{self, ConnectionExt},
    },
    xcb_ffi::XCBConnection,
};
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
#[derive(Clone)]
pub struct Output {
    pub data: DesktopOutput,
    pub rect: [i32; 4],
    pub scale: Scale,
    pub primary: bool,
    pub fullscreen_index: Option<u32>,
}
pub struct Displays {
    pub outputs: Vec<Output>,
    pub resource: u32,
    root: u32,
    modern: bool,
    mm: (u16, u16),
    overrides: BTreeMap<String, Scale>,
}
fn density(pixels: u32, mm: u32, dpi: Option<f64>) -> Scale {
    let dpi = dpi.unwrap_or_else(|| {
        if mm > 0 {
            f64::from(pixels) * 25.4 / f64::from(mm)
        } else {
            96.
        }
    });
    Scale::new(((dpi / 96. * 4.).round().clamp(4., 16.) as u32) * 30)
}
fn signed(scale: Scale, value: i32) -> i32 {
    if value < 0 {
        -(scale.logical(value.unsigned_abs()) as i32)
    } else {
        scale.logical(value as u32) as i32
    }
}
fn layout(outputs: &mut [Output]) {
    if outputs.is_empty() {
        return;
    }
    let anchor = outputs.iter().position(|o| o.primary).unwrap_or(0);
    let root = outputs[anchor].rect;
    let base = outputs[anchor].scale;
    let mut positions = vec![None; outputs.len()];
    positions[anchor] = Some((0i32, 0i32));
    loop {
        let mut progress = false;
        for a in 0..outputs.len() {
            let Some((x, y)) = positions[a] else {
                continue;
            };
            let r = outputs[a].rect;
            let size = outputs[a].data.logical_size;
            for b in 0..outputs.len() {
                if positions[b].is_some() {
                    continue;
                }
                let n = outputs[b].rect;
                let ns = outputs[b].data.logical_size;
                let pos = if n[0] == r[0] + r[2] {
                    Some((x + size.0 as i32, y + signed(outputs[a].scale, n[1] - r[1])))
                } else if n[0] + n[2] == r[0] {
                    Some((x - ns.0 as i32, y + signed(outputs[a].scale, n[1] - r[1])))
                } else if n[1] == r[1] + r[3] {
                    Some((x + signed(outputs[a].scale, n[0] - r[0]), y + size.1 as i32))
                } else if n[1] + n[3] == r[1] {
                    Some((x + signed(outputs[a].scale, n[0] - r[0]), y - ns.1 as i32))
                } else {
                    None
                };
                if let Some(pos) = pos {
                    positions[b] = Some(pos);
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
    for (i, o) in outputs.iter_mut().enumerate() {
        o.data.position = positions[i].unwrap_or((
            signed(base, o.rect[0] - root[0]),
            signed(base, o.rect[1] - root[1]),
        ));
    }
}
impl Displays {
    pub fn new(c: &XCBConnection, screen: usize) -> io::Result<Self> {
        let root = &c.setup().roots[screen];
        let resource = c
            .intern_atom(false, b"RESOURCE_MANAGER")
            .map_err(error)?
            .reply()
            .map_err(error)?
            .atom;
        let version = randr::query_version(c, 1, 5)
            .ok()
            .and_then(|v| v.reply().ok());
        let modern = version
            .is_some_and(|v| v.major_version > 1 || (v.major_version == 1 && v.minor_version >= 5));
        if version.is_some() {
            let mut mask = randr::NotifyMask::SCREEN_CHANGE
                | randr::NotifyMask::CRTC_CHANGE
                | randr::NotifyMask::OUTPUT_CHANGE;
            if modern {
                mask |= randr::NotifyMask::RESOURCE_CHANGE;
            }
            randr::select_input(c, root.root, mask)
                .map_err(error)?
                .check()
                .map_err(error)?;
        }
        let old = c
            .get_window_attributes(root.root)
            .map_err(error)?
            .reply()
            .map_err(error)?
            .your_event_mask;
        c.change_window_attributes(
            root.root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(
                old | xproto::EventMask::PROPERTY_CHANGE | xproto::EventMask::STRUCTURE_NOTIFY,
            ),
        )
        .map_err(error)?;
        let mut overrides = BTreeMap::new();
        if let Ok(value) = std::env::var("TRAMES_DESKTOP_OUTPUT_SCALES") {
            for setting in value.split(',').filter(|s| !s.is_empty()) {
                let (name, value) = setting.split_once('=').ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "output scale requires NAME=1.25",
                    )
                })?;
                let value = value.parse::<f64>().map_err(error)?;
                if !value.is_finite() || !(1.0..=4.0).contains(&value) {
                    return Err(error("output scale must be between 1 and 4"));
                }
                overrides.insert(name.to_string(), Scale::new((value * 120.).round() as u32));
            }
        }
        let mut s = Self {
            outputs: vec![],
            resource,
            root: root.root,
            modern,
            mm: (root.width_in_millimeters, root.height_in_millimeters),
            overrides,
        };
        s.refresh(c)?;
        Ok(s)
    }
    pub fn refresh(&mut self, c: &XCBConnection) -> io::Result<()> {
        let bytes = c
            .get_property(
                false,
                self.root,
                self.resource,
                xproto::AtomEnum::STRING,
                0,
                65536,
            )
            .map_err(error)?
            .reply()
            .map_err(error)?
            .value;
        let text = String::from_utf8_lossy(&bytes);
        let dpi = text.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("Xft.dpi") {
                v.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0.)
            } else {
                None
            }
        });
        let mut monitors = if self.modern {
            randr::get_monitors(c, self.root, false)
                .map_err(error)?
                .reply()
                .map_err(error)?
                .monitors
        } else {
            vec![]
        };
        monitors.retain(|m| m.width > 0 && m.height > 0);
        if monitors.is_empty() {
            let g = c
                .get_geometry(self.root)
                .map_err(error)?
                .reply()
                .map_err(error)?;
            monitors.push(randr::MonitorInfo {
                name: 1,
                primary: true,
                automatic: true,
                x: 0,
                y: 0,
                width: g.width,
                height: g.height,
                width_in_millimeters: u32::from(self.mm.0),
                height_in_millimeters: u32::from(self.mm.1),
                outputs: vec![],
            });
        }
        let screens = xinerama::query_screens(c)
            .ok()
            .and_then(|v| v.reply().ok())
            .map(|v| v.screen_info)
            .unwrap_or_default();
        self.outputs = monitors
            .into_iter()
            .map(|m| {
                let name = if m.name == 1 {
                    "X11-0".into()
                } else {
                    String::from_utf8_lossy(
                        &c.get_atom_name(m.name)
                            .map_err(error)?
                            .reply()
                            .map_err(error)?
                            .name,
                    )
                    .into_owned()
                };
                let scale = self
                    .overrides
                    .get(&name)
                    .or(self.overrides.get("*"))
                    .copied()
                    .unwrap_or_else(|| density(u32::from(m.width), m.width_in_millimeters, dpi));
                let pixel_size = (u32::from(m.width), u32::from(m.height));
                Ok(Output {
                    data: DesktopOutput {
                        id: u64::from(m.name),
                        name: name.clone(),
                        description: name,
                        position: (0, 0),
                        pixel_size,
                        logical_size: (scale.logical(pixel_size.0), scale.logical(pixel_size.1)),
                        physical_mm: (
                            m.width_in_millimeters as i32,
                            m.height_in_millimeters as i32,
                        ),
                        scale_120: scale.0,
                        refresh_millihz: 60000,
                        transform: 0,
                    },
                    rect: [
                        i32::from(m.x),
                        i32::from(m.y),
                        i32::from(m.width),
                        i32::from(m.height),
                    ],
                    scale,
                    primary: m.primary,
                    fullscreen_index: screens
                        .iter()
                        .position(|v| {
                            v.x_org == m.x
                                && v.y_org == m.y
                                && v.width == m.width
                                && v.height == m.height
                        })
                        .map(|v| v as u32),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        layout(&mut self.outputs);
        Ok(())
    }
    pub fn descriptors(&self) -> Vec<DesktopOutput> {
        self.outputs.iter().map(|o| o.data.clone()).collect()
    }
    pub fn fullscreen_index(&self, id: u64) -> Option<u32> {
        self.outputs
            .iter()
            .find(|o| o.data.id == id)
            .and_then(|o| o.fullscreen_index)
    }
    pub fn initial_scale(&self) -> Scale {
        self.outputs
            .iter()
            .find(|o| o.primary)
            .or(self.outputs.first())
            .map(|o| o.scale)
            .unwrap_or_default()
    }
    pub fn membership(&self, rect: [i32; 4]) -> (Vec<u64>, Scale) {
        let mut ids = vec![];
        let mut scale = Scale::default();
        for o in &self.outputs {
            let r = o.rect;
            if rect[0] < r[0] + r[2]
                && rect[0] + rect[2] > r[0]
                && rect[1] < r[1] + r[3]
                && rect[1] + rect[3] > r[1]
            {
                ids.push(o.data.id);
                scale.0 = scale.0.max(o.scale.0);
            }
        }
        if ids.is_empty() {
            scale = self.initial_scale();
        }
        (ids, scale)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn output(id: u64, x: i32, w: u32, scale: u32, primary: bool) -> Output {
        Output {
            data: DesktopOutput {
                id,
                name: id.to_string(),
                description: String::new(),
                position: (0, 0),
                logical_size: (Scale(scale).logical(w), 600),
                pixel_size: (w, 600),
                physical_mm: (0, 0),
                scale_120: scale,
                refresh_millihz: 60000,
                transform: 0,
            },
            rect: [x, 0, w as i32, 600],
            scale: Scale(scale),
            primary,
            fullscreen_index: None,
        }
    }
    #[test]
    fn mixed_density_layout_has_no_artificial_gap() {
        let mut v = [
            output(1, 0, 1280, 120, true),
            output(2, 1280, 1600, 240, false),
        ];
        layout(&mut v);
        assert_eq!(v[1].data.position, (1280, 0));
        assert_eq!(v[1].data.logical_size.0, 800);
        let mut v = [
            output(2, -1600, 1600, 240, false),
            output(1, 0, 1280, 120, true),
        ];
        layout(&mut v);
        assert_eq!(v[0].data.position, (-800, 0));
    }
    #[test]
    fn native_font_dpi_and_missing_physical_size() {
        assert_eq!(density(1920, 0, None), Scale(120));
        assert_eq!(density(1920, 0, Some(144.)), Scale(180));
        assert_eq!(density(1920, 0, Some(192.)), Scale(240));
    }
}
