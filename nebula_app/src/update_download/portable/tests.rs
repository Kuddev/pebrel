use super::*;
use zip::{ZipWriter, write::SimpleFileOptions};

fn package(root: &Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
    let path = root.join("package.zip");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn staging_preserves_package_paths_and_hashes_without_touching_the_installation() {
    let root = tempfile::tempdir().unwrap();
    let path = package(root.path(), &[("pebrel.exe", b"MZfixture"), ("docs/a file.txt", b"notes")]);
    inspect(&mut File::open(&path).unwrap()).unwrap();
    let payload = extract(&path, root.path()).unwrap();
    assert_eq!(payload.files.len(), 2);
    for entry in payload.files {
        let data = std::fs::read(payload.directory.join(&entry.path)).unwrap();
        assert_eq!(entry.bytes, data.len() as u64);
        assert_eq!(
            entry.sha256,
            Sha256::digest(data).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
    }
    assert!(!root.path().join("pebrel.exe").exists());
}

#[test]
fn reservations_include_new_file_bytes_and_existing_file_backups() {
    let root = tempfile::tempdir().unwrap();
    let installation = root.path().join("app");
    std::fs::create_dir(&installation).unwrap();
    std::fs::write(installation.join("pebrel.exe"), b"older binary").unwrap();
    let package = package(root.path(), &[("pebrel.exe", b"MZnew"), ("docs/new.txt", b"notes")]);
    let installation = crate::platform::update_installation::canonical(&installation).unwrap();
    assert_eq!(reservations(&package, &installation).unwrap(), (10, 22));
    assert!(!root.path().join("portable").exists());
}

#[test]
fn unsafe_windows_names_are_rejected_before_extraction() {
    for name in [
        "../escape",
        "/absolute",
        "C:/escape",
        "runtime\\escape",
        "docs/../escape",
        "docs/a:stream",
        "docs/CON.txt",
        "docs/LPT1",
        "docs/a.",
        "docs/a ",
        "docs//a",
        ".pebrel-update.nebula-lock",
        "unins000.exe",
        "pebrel-distribution",
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = package(root.path(), &[("pebrel.exe", b"MZ"), (name, b"bad")]);
        assert!(extract(&path, root.path()).is_err(), "{name}");
        assert!(!root.path().join("portable").exists(), "unsafe ZIP must fail before staging");
    }
}

#[test]
fn case_collisions_missing_executables_and_file_parents_fail_closed() {
    for entries in [
        vec![("pebrel.exe", b"MZ".as_slice()), ("PEBREL.EXE", b"MZ".as_slice())],
        vec![("README.md", b"notes".as_slice())],
        vec![
            ("pebrel.exe", b"MZ".as_slice()),
            ("docs", b"file".as_slice()),
            ("docs/a", b"a".as_slice()),
        ],
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = package(root.path(), &entries);
        assert!(inspect(&mut File::open(path).unwrap()).is_err());
    }
}

#[test]
fn truncated_zip_and_non_pe_main_executable_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = package(root.path(), &[("pebrel.exe", b"html error page")]);
    assert!(extract(&path, root.path()).is_err());
    let file = File::options().write(true).open(&path).unwrap();
    file.set_len(12).unwrap();
    assert!(inspect(&mut File::open(path).unwrap()).is_err());
}

#[test]
fn file_count_limit_is_checked_before_staging() {
    let root = tempfile::tempdir().unwrap();
    let names: Vec<_> = (0..MAX_FILES).map(|index| format!("docs/{index}")).collect();
    let mut entries = vec![("pebrel.exe", b"MZ".as_slice())];
    entries.extend(names.iter().map(|name| (name.as_str(), b"x".as_slice())));
    let path = package(root.path(), &entries);
    assert!(extract(&path, root.path()).is_err());
    assert!(!root.path().join("portable").exists());
}

#[test]
fn declared_expansion_limits_reject_small_archives_with_oversized_entries() {
    for total in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path =
            package(root.path(), &[("pebrel.exe", b"MZ"), ("docs/a", b"a"), ("docs/b", b"b")]);
        let mut bytes = std::fs::read(&path).unwrap();
        let positions: Vec<_> = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(index, part)| (part == b"PK\x01\x02").then_some(index))
            .collect();
        let oversized =
            if total { 400 * 1024 * 1024 } else { super::super::MAX_INSTALLER_BYTES + 1 };
        for central in positions {
            bytes[central + 24..central + 28].copy_from_slice(&(oversized as u32).to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(extract(&path, root.path()).is_err());
        assert!(!root.path().join("portable").exists());
    }
}

#[test]
fn zip_symlinks_cannot_redirect_staged_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("symlink.zip");
    let mut writer = ZipWriter::new(File::create(&path).unwrap());
    writer.start_file("pebrel.exe", SimpleFileOptions::default()).unwrap();
    writer.write_all(b"MZ").unwrap();
    writer.add_symlink("docs", "../outside", SimpleFileOptions::default()).unwrap();
    writer.finish().unwrap();
    assert!(extract(&path, root.path()).is_err());
    assert!(!root.path().join("portable").exists());
}

#[test]
#[cfg(windows)]
fn archive_guard_prevents_replacement_or_writes_during_staging() {
    let root = tempfile::tempdir().unwrap();
    let path = package(root.path(), &[("pebrel.exe", b"MZ")]);
    let guard = lock_package(&path).unwrap();
    assert!(std::fs::write(&path, b"changed").is_err());
    assert!(std::fs::remove_file(&path).is_err());
    extract(&path, root.path()).unwrap();
    drop(guard);
    std::fs::write(&path, b"released").unwrap();
}
