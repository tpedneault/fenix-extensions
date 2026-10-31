//! The extension's own copy of a session's framebuffer, BGRA, as the
//! surface shows it: kept so a screenshot can be taken, and so a session
//! shown again (a new surface, after `SPC v v`) is drawn whole at once
//! rather than black until the guest next changes.
//!
//! Every write is cut to the buffer: a server sending a rectangle that
//! runs past its edge, or one out of step for a moment, mustn't take the
//! extension down over it.

/// Copies `src` (tightly-packed BGRA, `width` x `height`) into `dest`, a
/// `dest_width`-wide BGRA buffer, at (`x`, `y`), cut to `dest`'s bounds.
// Eight plain numbers in the order a VNC rectangle arrives in.
#[allow(clippy::too_many_arguments)]
pub fn blit_rect(dest: &mut [u8], dest_width: u16, dest_height: u16, x: u16, y: u16, width: u16, height: u16, src: &[u8]) {
    if x >= dest_width || y >= dest_height {
        return;
    }
    let copy_width = width.min(dest_width - x) as usize;
    let copy_height = height.min(dest_height - y) as usize;
    let src_stride = width as usize * 4;
    let dest_stride = dest_width as usize * 4;
    for row in 0..copy_height {
        let src_start = row * src_stride;
        let Some(src_row) = src.get(src_start..src_start + copy_width * 4) else { break };
        let dest_start = (y as usize + row) * dest_stride + x as usize * 4;
        let Some(dest_row) = dest.get_mut(dest_start..dest_start + copy_width * 4) else { break };
        dest_row.copy_from_slice(src_row);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Framebuffer {
    pixels: Vec<u8>,
    width: u16,
    height: u16,
}

impl Framebuffer {
    pub fn new() -> Framebuffer {
        Framebuffer::default()
    }

    /// `width` x `height`, black -- on a new resolution.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.pixels = [0, 0, 0, 255].repeat(width as usize * height as usize);
    }

    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// BGRA, row after row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// One rectangle of pixels (see `blit_rect`).
    pub fn apply_rect(&mut self, x: u16, y: u16, width: u16, height: u16, bgra: &[u8]) {
        blit_rect(&mut self.pixels, self.width, self.height, x, y, width, height, bgra);
    }

    /// A rectangle in one colour.
    pub fn fill(&mut self, x: u16, y: u16, width: u16, height: u16, (r, g, b): (u8, u8, u8)) {
        if x >= self.width || y >= self.height {
            return;
        }
        let w = width.min(self.width - x) as usize;
        let px = [b, g, r, 255].repeat(w);
        for row in y as usize..(y as usize + height as usize).min(self.height as usize) {
            let at = (row * self.width as usize + x as usize) * 4;
            self.pixels[at..at + w * 4].copy_from_slice(&px);
        }
    }

    /// RFB's `CopyRect`: the `width` x `height` pixels at (`src_x`,
    /// `src_y`) copied to (`x`, `y`). The two may overlap (a scroll), so
    /// the source is taken out first. Nothing before a resolution's known.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_copy(&mut self, x: u16, y: u16, width: u16, height: u16, src_x: u16, src_y: u16) {
        if self.width == 0 {
            return;
        }
        let stride = self.width as usize * 4;
        let row_bytes = width as usize * 4;
        let mut scratch = vec![0u8; row_bytes * height as usize];
        for row in 0..height as usize {
            let from = (src_y as usize + row) * stride + src_x as usize * 4;
            if let Some(bytes) = self.pixels.get(from..from + row_bytes) {
                scratch[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(bytes);
            }
        }
        blit_rect(&mut self.pixels, self.width, self.height, x, y, width, height, &scratch);
    }

    /// RGBA, for a PNG.
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
            px[3] = 255;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_rect(width: u16, height: u16, bgra: [u8; 4]) -> Vec<u8> {
        bgra.repeat(width as usize * height as usize)
    }

    fn pixel_at(pixels: &[u8], width: u16, x: u16, y: u16) -> [u8; 4] {
        let i = (y as usize * width as usize + x as usize) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    #[test]
    fn copies_a_rect_into_the_right_place_at_a_nonzero_offset() {
        let mut dest = vec![0u8; 4 * 4 * 4];
        blit_rect(&mut dest, 4, 4, 1, 1, 2, 2, &solid_rect(2, 2, [10, 20, 30, 255]));
        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            assert_eq!(pixel_at(&dest, 4, x, y), [10, 20, 30, 255]);
        }
        assert_eq!(pixel_at(&dest, 4, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel_at(&dest, 4, 3, 3), [0, 0, 0, 0]);
    }

    #[test]
    fn a_rect_starting_outside_the_buffer_is_dropped_entirely_not_a_panic() {
        let mut dest = vec![0u8; 4 * 4 * 4];
        blit_rect(&mut dest, 4, 4, 10, 10, 2, 2, &solid_rect(2, 2, [1, 2, 3, 4]));
        assert!(dest.iter().all(|&b| b == 0));
    }

    #[test]
    fn a_rect_overrunning_the_buffers_edge_is_clamped_not_a_panic() {
        let mut dest = vec![0u8; 4 * 4 * 4];
        blit_rect(&mut dest, 4, 4, 2, 2, 4, 4, &solid_rect(4, 4, [5, 6, 7, 8]));
        assert_eq!(pixel_at(&dest, 4, 2, 2), [5, 6, 7, 8]);
        assert_eq!(pixel_at(&dest, 4, 3, 3), [5, 6, 7, 8]);
        assert_eq!(dest.len(), 4 * 4 * 4);
    }

    #[test]
    fn a_truncated_source_buffer_stops_early_instead_of_panicking() {
        let mut dest = vec![0u8; 4 * 4 * 4];
        blit_rect(&mut dest, 4, 4, 0, 0, 2, 2, &solid_rect(2, 1, [9, 9, 9, 9]));
        assert_eq!(pixel_at(&dest, 4, 1, 0), [9, 9, 9, 9]);
        assert_eq!(pixel_at(&dest, 4, 0, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn new_starts_empty() {
        let fb = Framebuffer::new();
        assert_eq!(fb.dimensions(), (0, 0));
        assert!(fb.pixels().is_empty() && fb.is_empty());
    }

    #[test]
    fn resize_reallocates_black() {
        let mut fb = Framebuffer::new();
        fb.resize(4, 4);
        fb.apply_rect(0, 0, 2, 2, &solid_rect(2, 2, [1, 2, 3, 4]));
        fb.resize(2, 2);
        assert_eq!(fb.dimensions(), (2, 2));
        assert_eq!(fb.pixels(), &[0, 0, 0, 255].repeat(4)[..]);
    }

    #[test]
    fn apply_rect_before_any_resize_is_a_safe_no_op() {
        let mut fb = Framebuffer::new();
        fb.apply_rect(0, 0, 2, 2, &solid_rect(2, 2, [1, 2, 3, 4]));
        assert!(fb.pixels().is_empty());
    }

    #[test]
    fn fill_paints_a_clamped_rectangle() {
        let mut fb = Framebuffer::new();
        fb.resize(4, 4);
        fb.fill(2, 2, 9, 9, (10, 20, 30));
        assert_eq!(pixel_at(fb.pixels(), 4, 3, 3), [30, 20, 10, 255], "BGRA");
        assert_eq!(pixel_at(fb.pixels(), 4, 1, 1), [0, 0, 0, 255]);
        fb.fill(9, 9, 1, 1, (1, 1, 1));
    }

    #[test]
    fn apply_copy_moves_pixels_from_src_to_dst() {
        let mut fb = Framebuffer::new();
        fb.resize(4, 4);
        fb.apply_rect(0, 0, 2, 2, &solid_rect(2, 2, [5, 6, 7, 8]));
        fb.apply_copy(2, 2, 2, 2, 0, 0);
        assert_eq!(pixel_at(fb.pixels(), 4, 2, 2), [5, 6, 7, 8]);
        assert_eq!(pixel_at(fb.pixels(), 4, 3, 3), [5, 6, 7, 8]);
        assert_eq!(pixel_at(fb.pixels(), 4, 0, 0), [5, 6, 7, 8], "the source stays");
    }

    #[test]
    fn apply_copy_handles_overlapping_src_and_dst_regions_correctly() {
        // Rows 0 and 1 copied one row down onto themselves: a naive
        // in-place copy would read row 1 after overwriting it.
        let mut fb = Framebuffer::new();
        fb.resize(3, 3);
        let mut src = Vec::new();
        for row in 0..3u8 {
            src.extend_from_slice(&[row, row, row, 255].repeat(3));
        }
        fb.apply_rect(0, 0, 3, 3, &src);
        fb.apply_copy(0, 1, 3, 2, 0, 0);
        assert_eq!(pixel_at(fb.pixels(), 3, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel_at(fb.pixels(), 3, 0, 1), [0, 0, 0, 255]);
        assert_eq!(pixel_at(fb.pixels(), 3, 0, 2), [1, 1, 1, 255]);
    }

    #[test]
    fn apply_copy_before_any_resolution_is_known_is_a_no_op() {
        let mut fb = Framebuffer::new();
        fb.apply_copy(0, 0, 2, 2, 2, 2);
        assert!(fb.pixels().is_empty());
    }

    #[test]
    fn rgba_swaps_red_and_blue_and_is_opaque() {
        let mut fb = Framebuffer::new();
        fb.resize(1, 1);
        fb.apply_rect(0, 0, 1, 1, &[1, 2, 3, 0]);
        assert_eq!(fb.rgba(), [3, 2, 1, 255]);
    }
}
