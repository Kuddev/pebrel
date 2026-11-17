use std::io::Read as _;

use super::*;

fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap().join("trusted");
    std::fs::create_dir(&root).unwrap();
    (temp, root)
}

#[test]
fn regular_file_reads_distinguish_missing_segments_from_invalid_roots() {
    let (_temp, root) = root();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(root.join("nested").join("会话.json"), b"owned data").unwrap();
    let mut file = open_regular_under_root(&root, &["nested", "会话.json"]).unwrap();
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    assert_eq!(text, "owned data");
    assert_eq!(
        probe_regular_under_root(&root, &["nested", "会话.json"]),
        RegularProbe::RegularFile
    );
    assert_eq!(probe_regular_under_root(&root, &["absent", "file"]), RegularProbe::NotFound);
    assert_eq!(
        probe_regular_under_root(&root.join("absent"), &["file"]),
        RegularProbe::InvalidOrError
    );
    assert_eq!(probe_regular_under_root(&root, &["nested"]), RegularProbe::InvalidOrError);
}

#[test]
fn relative_components_cannot_escape_the_verified_root() {
    let (_temp, root) = root();
    std::fs::write(root.parent().unwrap().join("outside"), b"outside data").unwrap();
    for parts in [
        vec![],
        vec![""],
        vec!["."],
        vec!["..", "outside"],
        vec!["../outside"],
        vec!["..\\outside"],
        vec!["/outside"],
        vec!["bad\0name"],
    ] {
        assert_eq!(
            probe_regular_under_root(&root, &parts),
            RegularProbe::InvalidOrError,
            "{parts:?}"
        );
    }
}

#[test]
fn linked_leaf_intermediate_root_and_ancestor_are_rejected() {
    use crate::platform::local_paths::{symlink_directory_for_test, symlink_file_for_test};
    let (_temp, root) = root();
    let parent = root.parent().unwrap();
    let actual = parent.join("actual");
    std::fs::create_dir(&actual).unwrap();
    std::fs::create_dir(actual.join("nested")).unwrap();
    std::fs::write(actual.join("nested").join("data"), b"outside data").unwrap();
    match symlink_file_for_test(&actual.join("nested").join("data"), &root.join("leaf")) {
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("Windows runner lacks symbolic-link privileges; link fixtures unavailable");
            return;
        },
        result => result.unwrap(),
    }
    symlink_directory_for_test(&actual, &root.join("link")).unwrap();
    assert_eq!(probe_regular_under_root(&root, &["leaf"]), RegularProbe::InvalidOrError);
    assert_eq!(
        probe_regular_under_root(&root, &["link", "nested", "data"]),
        RegularProbe::InvalidOrError
    );
    assert_eq!(
        probe_regular_under_root(&root.join("link"), &["nested", "data"]),
        RegularProbe::InvalidOrError
    );
    assert_eq!(
        probe_regular_under_root(&root.join("link").join("nested"), &["data"]),
        RegularProbe::InvalidOrError
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_without_a_writer_is_rejected_before_reading() {
    let (_temp, root) = root();
    create_fifo_for_test(&root.join("pipe")).unwrap().unwrap();
    let started = std::time::Instant::now();
    assert_eq!(probe_regular_under_root(&root, &["pipe"]), RegularProbe::InvalidOrError);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}
