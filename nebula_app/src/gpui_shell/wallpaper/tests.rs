use super::*;
use gpui::TestAppContext;

#[test]
fn letterbox_edges_stay_square_and_native_alignment_inherits_card_corners() {
    let card = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
    let letterbox = Bounds::new(point(px(0.0), px(100.0)), size(px(800.0), px(400.0)));
    assert_eq!(image_corners(card, letterbox, px(12.0)), Corners::all(px(0.0)));
    assert_eq!(image_corners(card, card, px(12.0)), Corners::all(px(12.0)));
    let native = Bounds::new(card.origin, size(px(400.0), px(300.0)));
    let corners = image_corners(card, native, px(12.0));
    assert_eq!(corners.top_left, px(12.0));
    assert_eq!(corners.top_right, px(0.0));
    assert_eq!(corners.bottom_left, px(0.0));
    assert_eq!(corners.bottom_right, px(0.0));
}

fn settings(path: Option<PathBuf>) -> nebula_settings::RuntimeSettings {
    let mut settings = nebula_settings::RuntimeSettings::load();
    settings.background_image = path.map(|path| path.to_string_lossy().into_owned());
    settings.background_image_opacity = 0.38;
    settings.background_image_fit = Some("fill".into());
    settings.background_image_alignment = Some("center".into());
    settings.background_image_cover_chrome = false;
    settings
}

fn fixture(path: &std::path::Path, color: [u8; 4]) {
    RgbaImage::from_pixel(64, 32, image::Rgba(color)).save(path).unwrap();
}

#[gpui::test]
fn loading_yields_and_layout_opacity_refreshes_reuse_the_same_image(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wallpaper.png");
    fixture(&path, [10, 20, 30, 255]);
    let mut rt = settings(Some(path));
    rt.background_image_cover_chrome = true;
    cx.update(|cx| {
        update_wallpaper(&rt, 1.0, BlurModeName::None, cx);
        let effects = cx.global::<VisualEffects>();
        assert!(effects.loading);
        assert!(effects.wallpaper.as_ref().unwrap().image.is_none());
        assert_eq!(chrome_surface_opacity(cx), 1.0);
    });
    cx.run_until_parked();
    let original = cx.update(|cx| {
        assert!(!cx.global::<VisualEffects>().loading);
        assert_eq!(chrome_surface_opacity(cx), 0.78);
        cx.global::<VisualEffects>().wallpaper.as_ref().unwrap().image.clone().unwrap()
    });
    rt.background_image_opacity = 0.75;
    rt.background_image_fit = Some("contain".into());
    rt.background_image_cover_chrome = true;
    cx.update(|cx| update_wallpaper(&rt, 0.8, BlurModeName::None, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        let wp = cx.global::<VisualEffects>().wallpaper.as_ref().unwrap();
        assert!(Arc::ptr_eq(&original, wp.image.as_ref().unwrap()));
        assert_eq!(wp.opacity, 0.75);
        assert_eq!(wp.fit, BackgroundImageFit::Uniform);
        assert_eq!(chrome_surface_opacity(cx), 0.78);
        for width in 800..1000 {
            let bounds = Bounds::new(point(px(100.0), px(30.0)), size(px(width as f32), px(600.0)));
            let _ = image_bounds(wp, bounds, 1.5);
            assert!(Arc::ptr_eq(&original, wp.image.as_ref().unwrap()));
        }
        rt.background_image_opacity = 0.0;
        update_wallpaper(&rt, 0.8, BlurModeName::None, cx);
        assert_eq!(chrome_surface_opacity(cx), 0.78);
        set_opacity_live(0.4, cx);
        assert_eq!(chrome_surface_opacity(cx), 0.4);
        rt.background_image_cover_chrome = false;
        update_wallpaper(&rt, 0.8, BlurModeName::None, cx);
        assert_eq!(chrome_surface_opacity(cx), 0.8);
        update_wallpaper(&settings(None), 0.8, BlurModeName::None, cx);
        assert_eq!(chrome_surface_opacity(cx), 0.8);
    });
}

#[gpui::test]
fn latest_request_wins_and_clearing_during_decode_cannot_restore_an_image(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.png");
    let last = directory.path().join("last.png");
    fixture(&first, [255, 0, 0, 255]);
    fixture(&last, [0, 0, 255, 255]);
    cx.update(|cx| {
        update_wallpaper(&settings(Some(first)), 1.0, BlurModeName::None, cx);
        update_wallpaper(&settings(Some(last.clone())), 1.0, BlurModeName::None, cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let wp = cx.global::<VisualEffects>().wallpaper.as_ref().unwrap();
        assert_eq!(wp.path, last);
        assert_eq!(&wp.image.as_ref().unwrap().as_bytes(0).unwrap()[..4], &[255, 0, 0, 255]);
        update_wallpaper(
            &settings(Some(directory.path().join("missing.png"))),
            1.0,
            BlurModeName::None,
            cx,
        );
        update_wallpaper(&settings(None), 1.0, BlurModeName::None, cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(cx.global::<VisualEffects>().wallpaper.is_none());
        assert!(!cx.global::<VisualEffects>().loading);
    });
}

#[test]
fn card_and_chrome_share_the_window_anchor_and_preserve_native_physical_size() {
    let mut wp = Wallpaper {
        path: PathBuf::new(),
        image: None,
        stamp: None,
        width: 800,
        height: 400,
        fit: BackgroundImageFit::UniformToFill,
        alignment: BackgroundImageAlignment::Center,
        cover_chrome: true,
        opacity: 0.38,
    };
    let anchor = Bounds::new(point(px(0.0), px(0.0)), size(px(600.0), px(600.0)));
    assert_eq!(
        image_bounds(&wp, anchor, 1.0),
        Bounds::new(point(px(-300.0), px(0.0)), size(px(1200.0), px(600.0)))
    );
    wp.fit = BackgroundImageFit::Uniform;
    assert_eq!(
        image_bounds(&wp, anchor, 1.0),
        Bounds::new(point(px(0.0), px(150.0)), size(px(600.0), px(300.0)))
    );
    wp.fit = BackgroundImageFit::None;
    assert_eq!(image_bounds(&wp, anchor, 2.0).size, size(px(400.0), px(200.0)));
    wp.alignment = BackgroundImageAlignment::BottomRight;
    assert_eq!(image_bounds(&wp, anchor, 2.0).origin, point(px(200.0), px(400.0)));
}
