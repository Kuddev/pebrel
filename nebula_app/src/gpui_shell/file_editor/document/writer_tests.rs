use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn independently_loaded_local_drafts_cannot_publish_in_the_same_cas_gap() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shared.rs");
    fs::write(&path, "original").unwrap();
    let first = Document::load(&path).unwrap();
    let second = Document::load(&path).unwrap();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let writer = std::thread::spawn({
        let path = path.clone();
        move || {
            first.save_before_publish(&path, "first draft".into(), || {
                entered_tx.send(()).expect("report entry into the publication gap");
                release_rx.recv_timeout(Duration::from_secs(5)).expect("release the paused writer");
            })
        }
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(5));
    let contender = entered.as_ref().ok().map(|_| second.save(&path, "second draft".into()));
    // Always release and join before asserting, even if readiness failed or
    // the mutation removes the permit. Neither channel wait is unbounded.
    let released = release_tx.send(());
    let saved = writer.join();
    entered.expect("writer must reach the publication gap");
    released.expect("writer must remain paused until released");
    let saved = saved.unwrap().unwrap();
    assert!(matches!(contender, Some(Err(SaveError::Busy))));
    assert_eq!(fs::read_to_string(&path).unwrap(), "first draft");
    assert!(matches!(second.save(&path, "second draft".into()), Err(SaveError::Changed)));
    saved.save(&path, "next confirmed draft".into()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "next confirmed draft");
}

#[test]
fn local_precondition_error_releases_the_target_for_a_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retry.txt");
    fs::write(&path, "original").unwrap();
    let document = Document::load(&path).unwrap();
    // Replacing the file with a directory forces the first precondition to
    // fail. The original snapshot can retry after the exact bytes return.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(document.save(&path, "draft".into()).is_err());
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "original").unwrap();
    document.save(&path, "draft".into()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "draft");
}

#[test]
fn independently_loaded_symlink_aliases_share_the_same_writer_permit() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.md");
    let alias = directory.path().join("alias.md");
    fs::write(&target, "original").unwrap();
    if let Err(error) = crate::platform::local_paths::symlink_file_for_test(&target, &alias) {
        if crate::platform::Platform::current() == crate::platform::Platform::Windows
            && error.kind() == std::io::ErrorKind::PermissionDenied
        {
            crate::gpui_shell::try_write_stderr(format_args!(
                "Windows runner lacks permission to create synthetic symlink; canonical alias test skipped"
            ));
            return;
        }
        panic!("synthetic alias creation failed: {error}");
    }
    let document = Document::load(&alias).unwrap();
    let _permit = writers::acquire(&target.canonicalize().unwrap()).unwrap();
    assert!(matches!(document.save(&alias, "draft".into()), Err(SaveError::Busy)));
    assert!(alias.is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "original");
}

#[cfg(feature = "gpui-test-support")]
#[gpui::test]
fn a_busy_writer_keeps_the_real_editor_draft_dirty_and_can_retry(cx: &mut gpui::TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("editor.txt");
    fs::write(&path, "original").unwrap();
    let (file, mut window) = super::super::tests::open(path.clone(), cx);
    window.update(|window, cx| {
        file.update(cx, |file, cx| {
            file.preview = false;
            file.input.update(cx, |input, cx| input.replace_all("unsaved draft", window, cx));
        });
    });
    window.run_until_parked();
    file.read_with(&window, |file, cx| {
        assert!(
            file.dirty && !file.saving && !file.loading,
            "source edit must reach a ready dirty editor: dirty={} saving={} loading={}",
            file.dirty,
            file.saving,
            file.loading
        );
        assert_eq!(file.draft(cx), "unsaved draft");
    });
    let permit = writers::acquire(&path.canonicalize().unwrap()).unwrap();
    window.update(|_, cx| file.update(cx, |file, cx| file.save(cx).detach()));
    window.run_until_parked();
    file.read_with(&window, |file, cx| {
        assert!(
            file.dirty && !file.saving,
            "busy save must retain dirty draft and finish: dirty={} saving={}",
            file.dirty,
            file.saving
        );
        assert_eq!(file.draft(cx), "unsaved draft");
        assert!(matches!(file.notice, Some((crate::i18n::Message::EditorSaveBusy, None))));
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    drop(permit);
    window.update(|_, cx| file.update(cx, |file, cx| file.save(cx).detach()));
    window.run_until_parked();
    file.read_with(&window, |file, cx| {
        assert!(!file.dirty && !file.saving);
        assert_eq!(file.draft(cx), "unsaved draft");
        assert!(matches!(file.notice, Some((crate::i18n::Message::EditorSaved, None))));
    });
    assert_eq!(fs::read_to_string(path).unwrap(), "unsaved draft");
}
