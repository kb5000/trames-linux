// SPDX-License-Identifier: GPL-3.0-or-later
//! Display coordinates are logical; native buffers are measured in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale(pub u32);
impl Default for Scale {
    fn default() -> Self {
        Self(120)
    }
}
impl Scale {
    pub fn new(value: u32) -> Self {
        Self(value.clamp(120, 480))
    }
    pub fn pixels(self, value: u32) -> u32 {
        ((u64::from(value) * u64::from(self.0) + 60) / 120).min(u64::from(u32::MAX)) as u32
    }
    pub fn logical(self, value: u32) -> u32 {
        ((u64::from(value) * 120 + u64::from(self.0) / 2) / u64::from(self.0))
            .min(u64::from(u32::MAX)) as u32
    }
    pub fn size(self, size: (u32, u32)) -> (u32, u32) {
        (self.pixels(size.0), self.pixels(size.1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_dimensions_round_once_and_input_stays_logical() {
        for (s, px) in [(120, 240), (150, 300), (180, 360), (240, 480)] {
            let scale = Scale(s);
            assert_eq!(scale.pixels(240), px);
            assert_eq!(scale.logical(px), 240);
        }
        assert_eq!(Scale::new(0), Scale::default());
        assert_eq!(Scale::new(150).size((240, 160)), (300, 200));
        assert_eq!(Scale(150).pixels(243), 304);
        assert_eq!(Scale(150).logical(304), 243);
    }
}
