use super::*;
fn png(width: u32, height: u32, noise: bool) -> Vec<u8> {
    let mut bytes = vec![0; width as usize * height as usize * 4];
    let mut seed = 0x1254_ae31_u32;
    for pixel in bytes.chunks_exact_mut(4) {
        for value in &mut pixel[..3] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *value = if noise { seed as u8 } else { 0x73 };
        }
        pixel[3] = 255;
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&bytes, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    encoded
}
#[test]
fn dpi_scaled_work_viewports_fit_the_same_person_frame() {
    for dpi in [96_u32, 120, 144, 192, 288] {
        let bounds = (1280 * dpi / 96, 800 * dpi / 96);
        assert!(input_dimensions(bounds.0, bounds.1));
        assert_eq!(output_dimensions(bounds.0, bounds.1), Some((640, 400)));
    }
    assert!(!input_dimensions(5120, 3200));
    assert!(!input_dimensions(0, 800));
}
#[test]
fn actual_150_percent_high_entropy_png_is_small_and_preserves_pixels() {
    let input = png(1920, 1200, true);
    let started = std::time::Instant::now();
    let (width, height, small) = thumbnail(input, (1920, 1200)).unwrap();
    let conversion = started.elapsed();
    assert_eq!((width, height), (640, 400));
    assert!(small.len() <= OUTPUT_PNG_BYTES);
    let decoded = image::load_from_memory(&small).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (640, 400));
    assert!(decoded.pixels().all(|pixel| pixel[3] == 255));
    assert!(decoded.pixels().any(|pixel| pixel[0] != pixel[1]));
    let _ = writeln!(
        std::io::stdout().lock(),
        "work-frame-image-test: source_dpi=150 elapsed_ms={} output_bytes={}",
        conversion.as_millis(),
        small.len()
    );
}
#[test]
fn source_identity_dimensions_and_raw_bounds_are_not_relaxed() {
    assert!(thumbnail(png(4, 2, false), (4, 3)).is_none());
    assert!(thumbnail(vec![0; RAW_PNG_BYTES + 1], (1280, 800)).is_none());
    let mut invalid = png(4, 2, false);
    invalid[16..20].copy_from_slice(&5000_u32.to_be_bytes());
    assert!(thumbnail(invalid, (5000, 2)).is_none());
    let mut invalid = png(4, 2, false);
    invalid[24] = 16;
    assert!(thumbnail(invalid, (4, 2)).is_none());
}
#[test]
fn output_sink_refuses_before_growth_and_keeps_exact_limit() {
    let mut sink = OutputSink(Vec::new());
    sink.write_all(&vec![0; OUTPUT_PNG_BYTES]).unwrap();
    assert!(sink.write_all(&[1]).is_err());
    assert_eq!(sink.0.len(), OUTPUT_PNG_BYTES);
    assert!(sink.0.capacity() <= OUTPUT_PNG_BYTES);
}

#[test]
fn fractional_125_percent_area_preserves_a_centered_impulse() {
    let mut source = vec![0_u8; 5 * 4];
    for pixel in source.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    source[8..11].fill(255);
    let actual = area_rgba(&source, (5, 1), 4, (2, 1)).unwrap();
    assert_eq!(actual, vec![51, 51, 51, 255, 51, 51, 51, 255]);
    // Fully transparent color cannot bleed into the visible edge.
    let alpha = area_rgba(&[255, 0, 0, 0, 0, 0, 255, 255], (2, 1), 4, (1, 1)).unwrap();
    assert_eq!(alpha, vec![0, 0, 255, 128]);
}
#[test]
fn high_entropy_square_cannot_escape_the_encoded_byte_budget() {
    assert!(thumbnail(png(640, 640, true), (640, 640)).is_none());
}
