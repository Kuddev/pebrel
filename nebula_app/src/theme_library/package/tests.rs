use super::*;
use crate::theme_library::{ThemeDocument, ThemeLibraryStore, builtin_document};
use nebula_settings::ThemeName;
use std::fs;
use std::io::Write;
use std::path::Path;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn author() -> Author {
    Author { name: "Community author".into(), github: Some("creator".into()) }
}

fn make_theme(image: &Path) -> ThemeDocument {
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Portable theme".into();
    definition.effects.background_image = Some(image.to_str().unwrap().into());
    definition.effects.background_image_opacity = Some(0.35);
    crate::theme_library::from_definition(&definition).unwrap()
}

#[test]
fn image_theme_round_trips_with_author_and_installs_without_applying() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("背景.png");
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255])).save(&image).unwrap();
    let theme = make_theme(&image);
    let output = dir.path().join("sample.pebrel-theme.zip");
    export_package(&theme, author(), "1.0.0".into(), "CC-BY-4.0".into(), &output, None).unwrap();
    fs::remove_file(&image).unwrap();
    let package = CheckedPackage::open(&output).unwrap();
    assert_eq!(package.manifest.author.name, "Community author");
    assert_eq!(
        package.document.definition().unwrap().effects.background_image.as_deref(),
        Some("assets/background.png")
    );
    let store = ThemeLibraryStore::new(dir.path().join("library"));
    let installed = package.install(&store).unwrap();
    assert_eq!(installed.definition().unwrap().effects.background_image_opacity, Some(0.35));
    let installed_path = installed.definition().unwrap().effects.background_image.unwrap();
    assert!(Path::new(&installed_path).is_file());
    assert_eq!(store.list().unwrap().custom.len(), 1);
    assert_eq!(store.load(installed.id().unwrap()).unwrap(), installed);
}

#[test]
fn colors_only_packages_preserve_optional_effects_and_repeated_imports_are_independent() {
    let dir = tempfile::tempdir().unwrap();
    let theme = builtin_document(ThemeName::Nord).unwrap();
    let output = dir.path().join("colors.pebrel-theme.zip");
    export_package(&theme, author(), "1.0.0".into(), "MIT".into(), &output, None).unwrap();
    let checked = CheckedPackage::open(&output).unwrap();
    assert!(checked.manifest.resources.is_empty());
    assert_eq!(checked.document.definition().unwrap().effects, theme.definition().unwrap().effects);
    let store = ThemeLibraryStore::new(dir.path().join("library"));
    let first = checked.install(&store).unwrap();
    let second = CheckedPackage::open(&output).unwrap().install(&store).unwrap();
    assert_ne!(first.id(), second.id());
    assert_ne!(first.name(), second.name());
}

fn resource(path: &str, kind: ResourceKind, bytes: u64) -> Resource {
    Resource { path: path.into(), kind, bytes, sha256: "a".repeat(64) }
}

fn manifest(resources: Vec<Resource>) -> Manifest {
    Manifest {
        package_version: 1,
        name: "Example".into(),
        author: author(),
        version: "1.0.0".into(),
        license: "MIT".into(),
        theme: THEME.into(),
        resources,
    }
}

#[test]
fn single_and_total_video_limits_are_exact_and_cannot_be_split_around() {
    assert!(
        manifest(vec![resource("assets/video.mp4", ResourceKind::Video, MAX_VIDEO_BYTES)])
            .validate()
            .is_ok()
    );
    assert!(
        manifest(vec![resource("assets/video.mp4", ResourceKind::Video, MAX_VIDEO_BYTES + 1)])
            .validate()
            .is_err()
    );
    assert!(
        manifest(vec![
            resource("assets/one.mp4", ResourceKind::Video, MAX_VIDEO_BYTES / 2),
            resource("assets/two.mp4", ResourceKind::Video, MAX_VIDEO_BYTES / 2 + 1)
        ])
        .validate()
        .is_err()
    );
}

#[test]
fn portable_paths_reject_traversal_drives_device_names_and_case_collisions() {
    for path in [
        "../outside",
        "/outside",
        "C:/outside",
        "assets/a\\b",
        "assets/CON.png",
        "assets/LPT1",
        "assets/trailing.",
        "assets//a",
        "assets/./a",
    ] {
        assert!(validate_path(path).is_err(), "{path}");
    }
    assert!(validate_path("assets/雪山.png").is_ok());
    assert!(
        manifest(vec![
            resource("assets/Image.png", ResourceKind::Image, 1),
            resource("assets/image.png", ResourceKind::Image, 1)
        ])
        .validate()
        .is_err()
    );
}

fn raw_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn undeclared_files_and_absolute_backgrounds_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Example".into();
    definition.effects.background_image = Some("C:/private/image.png".into());
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let path = dir.path().join("bad.zip");
    raw_zip(
        &path,
        &[
            (MANIFEST, serde_json::to_vec(&manifest(vec![])).unwrap()),
            (THEME, document.to_json_bytes().unwrap()),
        ],
    );
    assert!(CheckedPackage::open(&path).is_err());
    definition.effects.background_image = None;
    let document = crate::theme_library::from_definition(&definition).unwrap();
    raw_zip(
        &path,
        &[
            (MANIFEST, serde_json::to_vec(&manifest(vec![])).unwrap()),
            (THEME, document.to_json_bytes().unwrap()),
            ("assets/undeclared.png", vec![1]),
        ],
    );
    assert!(CheckedPackage::open(&path).is_err());
}

#[test]
fn archive_limit_is_checked_before_zip_metadata_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.zip");
    fs::File::create(&path).unwrap().set_len(MAX_ARCHIVE_BYTES + 1).unwrap();
    let error = match CheckedPackage::open(&path) {
        Ok(_) => panic!("oversized ZIP accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("48 MiB"));
}

#[test]
fn failed_exports_preserve_existing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("existing.zip");
    fs::write(&output, b"keep existing bytes").unwrap();
    let theme = make_theme(&dir.path().join("missing.png"));
    assert!(export_package(&theme, author(), "1.0.0".into(), "MIT".into(), &output, None).is_err());
    assert_eq!(fs::read(output).unwrap(), b"keep existing bytes");
}

#[test]
fn wrong_resource_hash_and_disguised_video_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("small.png");
    image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])).save(&image_path).unwrap();
    let payload = fs::read(image_path).unwrap();
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Example".into();
    definition.effects.background_image = Some("assets/image.png".into());
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let manifest =
        manifest(vec![resource("assets/image.png", ResourceKind::Image, payload.len() as u64)]);
    let path = dir.path().join("bad-hash.zip");
    raw_zip(
        &path,
        &[
            (MANIFEST, serde_json::to_vec(&manifest).unwrap()),
            (THEME, document.to_json_bytes().unwrap()),
            ("assets/image.png", payload),
        ],
    );
    assert!(CheckedPackage::open(&path).is_err());
    let mut disguised = manifest;
    disguised.resources[0].path = "assets/video.mp4".into();
    assert!(disguised.validate().is_err(), "video cannot claim the image allowance");
}

#[test]
fn busy_library_rolls_back_extracted_resources() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("image.png");
    image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])).save(&image).unwrap();
    let output = dir.path().join("theme.zip");
    export_package(&make_theme(&image), author(), "1.0.0".into(), "MIT".into(), &output, None)
        .unwrap();
    let store = ThemeLibraryStore::new(dir.path().join("library"));
    let _lock = crate::atomic_file::try_lifetime_lock(&store.root().join(".pebrel-theme-library"))
        .unwrap()
        .unwrap();
    assert!(CheckedPackage::open(&output).unwrap().install(&store).is_err());
    assert_eq!(fs::read_dir(store.root().join("packages")).unwrap().count(), 0);
    assert!(store.list().unwrap().custom.is_empty());
}

#[test]
fn directory_and_traversal_zip_entries_are_rejected_before_installation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("traversal.zip");
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Example".into();
    let document = crate::theme_library::from_definition(&definition).unwrap();
    raw_zip(
        &path,
        &[
            (MANIFEST, serde_json::to_vec(&manifest(vec![])).unwrap()),
            (THEME, document.to_json_bytes().unwrap()),
            ("../outside.txt", vec![1]),
        ],
    );
    assert!(CheckedPackage::open(&path).is_err());
    assert!(!dir.path().join("outside.txt").exists());
}

#[test]
fn optional_preview_is_portable_and_installed_alongside_background() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("image.png");
    let preview = dir.path().join("preview.jpg");
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255])).save(&image).unwrap();
    image::RgbImage::from_pixel(2, 1, image::Rgb([10, 20, 30])).save(&preview).unwrap();
    let output = dir.path().join("preview.zip");
    export_package(
        &make_theme(&image),
        author(),
        "1.0.0".into(),
        "MIT".into(),
        &output,
        Some(&preview),
    )
    .unwrap();
    let checked = CheckedPackage::open(&output).unwrap();
    assert_eq!(checked.manifest.resources.len(), 2);
    assert_eq!(checked.manifest.resources[1].kind, ResourceKind::Preview);
    let store = ThemeLibraryStore::new(dir.path().join("library"));
    let installed = checked.install(&store).unwrap();
    let background = installed.definition().unwrap().effects.background_image.unwrap();
    assert!(Path::new(&background).is_absolute());
    assert_eq!(
        fs::read(Path::new(&background).parent().unwrap().join("preview.jpg")).unwrap(),
        fs::read(preview).unwrap()
    );
}

#[test]
fn forged_directory_count_and_zip64_extra_are_rejected_before_parsing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("ordinary.zip");
    export_package(
        &builtin_document(ThemeName::Nord).unwrap(),
        author(),
        "1.0.0".into(),
        "MIT".into(),
        &output,
        None,
    )
    .unwrap();
    let original = fs::read(&output).unwrap();
    let end = original.len() - 22;
    let mut bytes = original.clone();
    bytes[end + 8..end + 10].copy_from_slice(&64u16.to_le_bytes());
    bytes[end + 10..end + 12].copy_from_slice(&64u16.to_le_bytes());
    fs::write(&output, bytes).unwrap();
    assert!(CheckedPackage::open(&output).is_err());
    let directory = u32::from_le_bytes(original[end + 16..end + 20].try_into().unwrap()) as usize;
    let name_len =
        u16::from_le_bytes(original[directory + 28..directory + 30].try_into().unwrap()) as usize;
    assert_eq!(&original[directory + 30..directory + 32], &[0, 0]);
    let mut bytes = original;
    bytes[directory + 30..directory + 32].copy_from_slice(&4u16.to_le_bytes());
    bytes.splice(directory + 46 + name_len..directory + 46 + name_len, [1, 0, 0, 0]);
    let end = end + 4;
    let size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    bytes[end + 12..end + 16].copy_from_slice(&(size + 4).to_le_bytes());
    fs::write(&output, bytes).unwrap();
    let error = match CheckedPackage::open(&output) {
        Ok(_) => panic!("Zip64 accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("Zip64"));
}

#[test]
fn deflated_colors_package_is_accepted_but_unsupported_media_install_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("deflated.zip");
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Example".into();
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let mut writer = ZipWriter::new(fs::File::create(&output).unwrap());
    for (name, bytes) in [
        (MANIFEST, serde_json::to_vec(&manifest(vec![])).unwrap()),
        (THEME, document.to_json_bytes().unwrap()),
    ] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
    assert!(CheckedPackage::open(&output).is_ok());
    let mut value = document.to_value();
    value.as_object_mut().unwrap().remove("metadata");
    let metadata_free = ThemeDocument::from_value(value).unwrap();
    raw_zip(
        &output,
        &[
            (MANIFEST, serde_json::to_vec(&manifest(vec![])).unwrap()),
            (THEME, metadata_free.to_json_bytes().unwrap()),
        ],
    );
    let store = ThemeLibraryStore::new(dir.path().join("metadata-free-library"));
    let installed = CheckedPackage::open(&output).unwrap().install(&store).unwrap();
    assert_eq!(installed.to_value()["metadata"]["package"]["license"], "MIT");
    let payload = b"reserved video payload";
    let digest = sha2::Sha256::digest(payload);
    use sha2::Digest;
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut resource = resource("assets/video.mp4", ResourceKind::Video, payload.len() as u64);
    resource.sha256 = hash;
    raw_zip(
        &output,
        &[
            (MANIFEST, serde_json::to_vec(&manifest(vec![resource])).unwrap()),
            (THEME, document.to_json_bytes().unwrap()),
            ("assets/video.mp4", payload.to_vec()),
        ],
    );
    let store = ThemeLibraryStore::new(dir.path().join("library"));
    assert!(CheckedPackage::open(&output).unwrap().install(&store).is_err());
    assert!(!store.root().exists(), "unsupported media is rejected before extracting");
}

#[test]
fn malformed_outer_directory_cannot_fall_back_to_a_valid_embedded_package() {
    let dir = tempfile::tempdir().unwrap();
    let inner = dir.path().join("inner.zip");
    let document = builtin_document(ThemeName::Nord).unwrap();
    export_package(&document, author(), "1.0.0".into(), "MIT".into(), &inner, None).unwrap();
    let embedded = fs::read(&inner).unwrap();
    let outer = dir.path().join("outer.zip");
    raw_zip(
        &outer,
        &[
            (MANIFEST, b"malformed manifest".to_vec()),
            (THEME, b"malformed theme".to_vec()),
            ("assets/embedded.zip", embedded),
        ],
    );
    let mut bytes = fs::read(&outer).unwrap();
    let end = bytes.len() - 22;
    let directory = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    // AES without its extra field makes the general reader retry an earlier
    // footer, which would silently select the otherwise valid embedded ZIP.
    bytes[directory + 10..directory + 12].copy_from_slice(&99u16.to_le_bytes());
    fs::write(&outer, bytes).unwrap();
    assert_eq!(
        zip::ZipArchive::new(fs::File::open(&outer).unwrap()).unwrap().len(),
        2,
        "fixture reproduces the generic reader's fallback"
    );
    assert!(
        CheckedPackage::open(&outer).is_err(),
        "package validation must stay with the preflighted outer envelope"
    );
}

#[test]
fn repeated_central_directory_names_cannot_disappear_in_the_zip_index() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("duplicate.zip");
    let mut definition = builtin_document(ThemeName::Nord).unwrap().definition().unwrap();
    definition.name = "Example".into();
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let metadata = serde_json::to_vec(&manifest(vec![])).unwrap();
    raw_zip(
        &output,
        &[
            (MANIFEST, metadata.clone()),
            (THEME, document.to_json_bytes().unwrap()),
            ("manifest.tmpx", metadata),
        ],
    );
    let mut bytes = fs::read(&output).unwrap();
    let end = bytes.len() - 22;
    let mut offset = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    for _ in 0..2 {
        let word =
            |i| u16::from_le_bytes(bytes[offset + i..offset + i + 2].try_into().unwrap()) as usize;
        offset += 46 + word(28) + word(30) + word(32);
    }
    bytes[offset + 46..offset + 46 + MANIFEST.len()].copy_from_slice(MANIFEST.as_bytes());
    fs::write(&output, bytes).unwrap();
    assert_eq!(
        zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap().len(),
        2,
        "general ZIP index collapses duplicates"
    );
    assert!(CheckedPackage::open(&output).is_err());
}
