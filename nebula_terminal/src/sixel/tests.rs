use super::*;

fn image(body: &str) -> Option<SixelImage> {
    SixelImage::parse(format!("\x1bP9;1q{body}\x1b\\").into_bytes())
}

#[test]
fn rgb_bands_repeats_and_transparent_pixels_decode_exactly() {
    let image = image("\"1;1;6;6#1;2;100;0;0!2~#2;2;0;100;0!2~#3;2;0;0;100!2@").unwrap();
    let rgba = image.decode_rgba().unwrap();
    assert_eq!(image.dimensions(), (6, 6));
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[8..12], &[0, 255, 0, 255]);
    assert_eq!(&rgba[16..20], &[0, 0, 255, 255]);
    assert_eq!(&rgba[40..44], &[0, 0, 0, 0]);
}

#[test]
fn carriage_return_overlays_colors_and_next_band_advances_six_rows() {
    let image = image("#1;2;100;0;0!2@$#2;2;0;100;0!2A-#1~~").unwrap();
    let rgba = image.decode_rgba().unwrap();
    assert_eq!(image.dimensions(), (2, 12));
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[8..12], &[0, 255, 0, 255]);
    assert_eq!(&rgba[48..52], &[255, 0, 0, 255]);
}

#[test]
fn opaque_background_is_resolved_by_frontend_and_hls_uses_dec_semantics() {
    let image = SixelImage::parse(b"\x1bP9;0q\"1;1;2;6#1;1;120;50;100@\x1b\\".to_vec()).unwrap();
    let rgba = image.decode_rgba_with_background([10, 20, 30]).unwrap();
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert!(!image.transparent);
    assert_eq!(&rgba[4..8], &[10, 20, 30, 255]);
}

#[test]
fn raster_overrides_header_aspect_and_retains_declared_blank_area() {
    let image = SixelImage::parse(b"\x1bP2;1q\"1;1;10;12~\x1b\\".to_vec()).unwrap();
    assert_eq!(image.aspect, (1, 1));
    assert_eq!(image.dimensions(), (10, 12));
    assert_eq!(image.decode_rgba().unwrap().len(), 480);
}

#[test]
fn hostile_dimensions_repeats_palettes_and_truncation_are_rejected() {
    for body in [
        "!4294967295~",
        "!42949672960~",
        "\"1;1;4097;1~",
        "\"4294967295;1;1;6~",
        "#256~",
        "#1;2;101;0;0~",
        "#1;1;361;50;50~",
        "!10",
        "#1;2;3~",
        "~\x18",
        "~\x1a",
    ] {
        assert!(image(body).is_none(), "accepted {body:?}");
    }
    assert!(SixelImage::parse(b"\x1bPq~".to_vec()).is_none());
    assert!(image(&"-".repeat(684)).is_none());
    assert!(image(&"!4096~$".repeat(2731)).is_none());
    assert!(SixelImage::parse(vec![0; MAX_BYTES + 1]).is_none());
}

#[test]
fn graphics_queries_report_only_implemented_capabilities() {
    assert_eq!(graphics_reply(b"?2;1;0S").as_deref(), Some("\x1b[?2;0;4096;4096S"));
    assert_eq!(graphics_reply(b"?1;4S").as_deref(), Some("\x1b[?1;0;256S"));
    assert_eq!(graphics_reply(b"?2;3;800;600S").as_deref(), Some("\x1b[?2;3;0S"));
    assert_eq!(graphics_reply(b"?3;1S").as_deref(), Some("\x1b[?3;1;0S"));
    assert_eq!(graphics_reply(b"?2;9S").as_deref(), Some("\x1b[?2;2;0S"));
    assert!(graphics_reply(b"1S").is_none());
    assert!(graphics_reply(b"?2:1S").is_none());
}

#[test]
fn arbitrary_short_inputs_never_panic() {
    let mut state = 17u32;
    for _ in 0..2000 {
        let mut bytes = b"\x1bP9;1q".to_vec();
        for _ in 0..64 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            bytes.push((state >> 24) as u8);
        }
        bytes.extend_from_slice(b"\x1b\\");
        if let Some(image) = SixelImage::parse(bytes) {
            assert!(image.decode_rgba().is_some());
        }
    }
}

#[test]
fn formatting_controls_inside_header_and_parameters_are_ignored() {
    let normal = image("\"1;1;12;6#1;2;100;0;0!12~").unwrap();
    let formatted =
        SixelImage::parse(b"\x1bP9;\n1q\"1;1;12;6#1;2;1\r00;0;0!1\n2~\x1b\\".to_vec()).unwrap();
    assert_eq!(normal.dimensions(), formatted.dimensions());
    assert_eq!(normal.decode_rgba(), formatted.decode_rgba());
}

#[test]
fn final_partial_band_accepts_exactly_the_advertised_pixel_height() {
    let prefix = "-".repeat(682);
    for (sixel, height) in [('?', 4092), ('@', 4093), ('N', 4096)] {
        let image = image(&format!("{prefix}{sixel}")).unwrap();
        assert_eq!(image.dimensions(), (1, height));
        let rgba = image.decode_rgba().unwrap();
        assert_eq!(rgba.len(), height as usize * 4);
        assert!(rgba[..4092 * 4].iter().all(|byte| *byte == 0));
        for pixel in rgba[4092 * 4..].chunks_exact(4) {
            assert_eq!(pixel[3], 255);
        }
    }
    for sixel in ['^', '~'] {
        assert!(image(&format!("{prefix}{sixel}")).is_none());
    }
    let declared = image(&format!("\"1;1;2;4096{prefix}@")).unwrap();
    assert_eq!(declared.dimensions(), (2, 4096));
    assert_eq!(declared.decode_rgba().unwrap().len(), 2 * 4096 * 4);
}

#[test]
fn opaque_zero_bits_keep_background_bands_and_clip_only_unused_edge_pixels() {
    for (code, foreground) in [('?', false), ('@', true)] {
        let image = SixelImage::parse(format!("\x1bP9;0q{code}\x1b\\").into_bytes()).unwrap();
        assert_eq!(image.dimensions(), (1, 6));
        let pixels = image.decode_rgba_with_background([10, 20, 30]).unwrap();
        assert_eq!(&pixels[4..], &[10, 20, 30, 255].repeat(5));
        assert_eq!(&pixels[..4], if foreground { &[0, 0, 0, 255] } else { &[10, 20, 30, 255] });
    }
    for (code, accepted) in [('?', true), ('@', true), ('N', true), ('^', false), ('~', false)] {
        let image =
            SixelImage::parse(format!("\x1bP9;0q{}{code}\x1b\\", "-".repeat(682)).into_bytes());
        assert_eq!(image.is_some(), accepted);
        if let Some(image) = image {
            assert_eq!(image.dimensions(), (1, 4096));
            assert_eq!(image.decode_rgba().unwrap().len(), 4096 * 4);
        }
    }
}
