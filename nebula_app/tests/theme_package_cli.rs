//! Real CLI byte-budget checks. A valid MP4 free box sets the exact file size;
//! these test archive streaming and capability rejection, not playback cost.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::Command,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
const VIDEO_LIMIT: u64 = 32 * 1024 * 1024;

fn video(path: &Path, bytes: u64) {
    let sample = include_bytes!("fixtures/theme-package/minimal-video.mp4");
    let mut file = File::create(path).unwrap();
    file.write_all(sample).unwrap();
    let padding = bytes - sample.len() as u64;
    file.write_all(&(padding as u32).to_be_bytes()).unwrap();
    file.write_all(b"free").unwrap();
    let buffer = [0; 64 * 1024];
    let mut remaining = padding - 8;
    while remaining > 0 {
        let size = remaining.min(buffer.len() as u64) as usize;
        file.write_all(&buffer[..size]).unwrap();
        remaining -= size as u64;
    }
    assert_eq!(file.metadata().unwrap().len(), bytes);
}

fn hash(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn package(path: &Path, inputs: &[(&str, &Path)]) {
    let theme = json!({
        "schema_version":1, "name":"Video boundary fixture", "appearance":"dark",
        "terminal":{"background":"#20252c","foreground":"#e3e8ef","palette":vec!["#20252c";16]},
        "ui":{"derive":true,"background":"#20252c","sidebar":"#20252c","foreground":"#e3e8ef","muted":"#e3e8ef","accent":"#e3e8ef","border":"#e3e8ef","selected":"#e3e8ef","success":"#e3e8ef","warning":"#e3e8ef","danger":"#e3e8ef"},
        "typography":{"enabled":false,"ligatures":true},
        "layout":{"radius":8,"gutter":6,"divider_width":1},
        "effects":{"material":"solid","background_image":inputs[0].0}
    });
    let resources: Vec<_> = inputs.iter().map(|(name, file)| json!({
        "path":name,"kind":"video","bytes":fs::metadata(file).unwrap().len(),"sha256":hash(file)
    })).collect();
    let manifest = json!({"package_version":1,"name":"Video boundary fixture","author":{"name":"Fixture author"},"version":"1.0.0","license":"MIT","theme":"theme.pebrel-theme.json","resources":resources});
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in [("manifest.json", manifest), ("theme.pebrel-theme.json", theme)] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&serde_json::to_vec(&data).unwrap()).unwrap();
    }
    for (name, source) in inputs {
        zip.start_file(*name, options).unwrap();
        std::io::copy(&mut File::open(source).unwrap(), &mut zip).unwrap();
    }
    zip.finish().unwrap();
}

fn cli(config: &Path, verb: &str, path: &Path) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_pebrel"))
        .args(["theme", verb])
        .arg(path)
        .env("PEBREL_CONFIG_DIR", config)
        .env("NEBULA_CONFIG_DIR", config)
        .output()
        .expect("run native CLI");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "CLI output is not JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), json)
}

#[test]
fn video_bytes_are_enforced_at_the_native_cli_boundary_without_activating_media() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    fs::create_dir(&config).unwrap();
    let preferences = config.join("pebrel_settings.txt");
    let before = b"theme=Nord\nfont_size=15\n";
    fs::write(&preferences, before).unwrap();
    let file = root.path().join("video.mp4");
    let zip = root.path().join("video.pebrel-theme.zip");
    video(&file, VIDEO_LIMIT);
    package(&zip, &[("assets/video.mp4", &file)]);
    let (ok, checked) = cli(&config, "check", &zip);
    assert!(ok && checked["ok"] == true, "{checked}");
    assert_eq!(checked["result"]["manifest"]["resources"][0]["bytes"], VIDEO_LIMIT);
    let (ok, imported) = cli(&config, "import", &zip);
    assert!(!ok && imported["ok"] == false, "{imported}");
    assert!(imported["error"].as_str().unwrap().contains("capabilities not available"));
    assert!(!config.join("themes/packages").exists());
    video(&file, VIDEO_LIMIT + 1);
    package(&zip, &[("assets/video.mp4", &file)]);
    let (ok, checked) = cli(&config, "check", &zip);
    assert!(
        !ok && checked["error"].as_str().unwrap().contains("video exceeds 32 MiB"),
        "{checked}"
    );
    let second = root.path().join("second.mp4");
    video(&file, VIDEO_LIMIT / 2);
    video(&second, VIDEO_LIMIT / 2);
    package(&zip, &[("assets/video.mp4", &file), ("assets/second.mp4", &second)]);
    let (ok, checked) = cli(&config, "check", &zip);
    assert!(ok && checked["ok"] == true, "{checked}");
    video(&second, VIDEO_LIMIT / 2 + 1);
    package(&zip, &[("assets/video.mp4", &file), ("assets/second.mp4", &second)]);
    let (ok, checked) = cli(&config, "check", &zip);
    assert!(
        !ok && checked["error"].as_str().unwrap().contains("total video size exceeds 32 MiB"),
        "{checked}"
    );
    assert_eq!(fs::read(&preferences).unwrap(), before);
}
