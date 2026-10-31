//! A screenshot: the framebuffer as a PNG, encoded here.

/// `rgba` (`width` x `height`) as a PNG.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_decodes_back_to_its_pixels() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 9, 9, 9, 255];
        let bytes = png(2, 2, &rgba).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&buf[..16], &rgba);
    }

    #[test]
    fn the_wrong_number_of_pixels_is_an_error_not_a_panic() {
        assert!(png(2, 2, &[0; 4]).is_err());
    }
}
