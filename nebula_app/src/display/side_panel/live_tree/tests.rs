use super::super::{GitInfo, PanelSnapshot, PanelView, SidePanel};
use super::*;
use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "file tree did not converge before the deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn settle(panel: &mut SidePanel) {
    wait_until(|| {
        panel.sync(None);
        panel.tree_watch.last_refresh.is_some()
            && !panel.tree_watch.signal.dirty.load(Ordering::Acquire)
            && !panel.snapshot_running.load(Ordering::Acquire)
            && panel.snapshot_slot.lock().unwrap().is_none()
            && !panel.needs_refresh
    });
}

fn open(root: &Path) -> SidePanel {
    let mut panel = SidePanel::new();
    panel.toggle(PanelView::Files);
    panel.sync(Some(root.to_owned()));
    settle(&mut panel);
    panel
}

fn contains(panel: &SidePanel, path: &Path) -> bool {
    panel.file_rows().iter().any(|row| row.path == path)
}

fn expect_path(panel: &mut SidePanel, path: &Path, present: bool) {
    wait_until(|| {
        panel.sync(None);
        contains(panel, path) == present
    });
}

fn expand(panel: &mut SidePanel, path: &Path) {
    let index = panel.file_rows().iter().position(|row| row.path == path).unwrap();
    assert!(panel.click_row(index));
    settle(panel);
}

#[test]
fn clone_appears_without_navigation_or_explicit_refresh() {
    let source = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let hooks = tempfile::tempdir().unwrap();
    let git = |cwd: &Path, args: &[&str]| {
        let mut command = std::process::Command::new("git");
        // Fixture commits must not execute hooks from the user's global config.
        command.arg("-c").arg(format!("core.hooksPath={}", hooks.path().display()));
        command.current_dir(cwd).args(args);
        crate::platform::process::hidden_command(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    git(source.path(), &["init", "--quiet"]);
    std::fs::write(source.path().join("README.md"), "local clone fixture").unwrap();
    git(source.path(), &["add", "README.md"]);
    git(
        source.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let mut panel = open(directory.path());
    let clone = directory.path().join("new project 中文");
    git(
        directory.path(),
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            source.path().to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    expect_path(&mut panel, &clone, true);
    expand(&mut panel, &clone);
    assert!(contains(&panel, &clone.join("README.md")));
    assert!(!contains(&panel, &clone.join(".git")));
}

#[test]
fn expanded_tree_tracks_create_rename_delete_and_preserves_view_state() {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("expanded");
    std::fs::create_dir(&folder).unwrap();
    let kept = folder.join("keep.txt");
    std::fs::write(&kept, "keep").unwrap();
    let mut panel = open(directory.path());
    expand(&mut panel, &folder);
    panel.selected = Some(kept.clone());
    panel.scroll = 2;
    let vcs = Arc::new(GitInfo { branch: "unchanged-vcs".into(), ..Default::default() });
    panel.git = Some(vcs.clone());
    let created = folder.join("new.txt");
    std::fs::write(&created, "new").unwrap();
    expect_path(&mut panel, &created, true);
    let renamed = folder.join("renamed.txt");
    std::fs::rename(&created, &renamed).unwrap();
    expect_path(&mut panel, &renamed, true);
    assert!(!contains(&panel, &created));
    std::fs::remove_file(&renamed).unwrap();
    expect_path(&mut panel, &renamed, false);
    assert!(panel.expanded.contains(&folder));
    assert_eq!(panel.selected, Some(kept));
    assert_eq!(panel.scroll, 2);
    assert!(Arc::ptr_eq(panel.git.as_ref().unwrap(), &vcs), "tree events must not read VCS");
}

#[test]
fn replacing_root_rearms_notifications_for_later_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("replaceable");
    std::fs::create_dir(&root).unwrap();
    let mut panel = open(&root);
    std::fs::remove_dir(&root).unwrap();
    settle(&mut panel);
    std::fs::create_dir(&root).unwrap();
    let first = root.join("first.txt");
    std::fs::write(&first, "").unwrap();
    expect_path(&mut panel, &first, true);
    settle(&mut panel);
    let second = root.join("after-rearm.txt");
    std::fs::write(&second, "").unwrap();
    expect_path(&mut panel, &second, true);
}

#[cfg(unix)]
#[test]
fn canonical_notifications_follow_a_symlink_root_and_its_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    let link = directory.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::os::unix::fs::symlink(&first, &link).unwrap();
    let mut panel = open(&link);
    std::fs::write(first.join("before.txt"), "").unwrap();
    expect_path(&mut panel, &link.join("before.txt"), true);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second, &link).unwrap();
    std::fs::write(second.join("after.txt"), "").unwrap();
    expect_path(&mut panel, &link.join("after.txt"), true);
    settle(&mut panel);
    std::fs::write(second.join("later.txt"), "").unwrap();
    expect_path(&mut panel, &link.join("later.txt"), true);
}

#[test]
fn collapse_releases_descendant_watches_and_reexpand_catches_up() {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let mut panel = open(directory.path());
    expand(&mut panel, &folder);
    assert!(panel.tree_watch.scope.as_ref().unwrap().directories.contains(&folder));
    expand(&mut panel, &folder); // collapse the same row
    assert!(!panel.tree_watch.scope.as_ref().unwrap().directories.contains(&folder));
    let new_file = folder.join("while-collapsed.txt");
    std::fs::write(&new_file, "").unwrap();
    expand(&mut panel, &folder);
    assert!(contains(&panel, &new_file));
}

#[test]
fn tree_events_do_not_overwrite_search_results_or_restart_the_query() {
    let directory = tempfile::tempdir().unwrap();
    let matched = directory.path().join("needle.txt");
    std::fs::write(&matched, "").unwrap();
    let mut panel = open(directory.path());
    panel.set_file_search_query("needle".into());
    wait_until(|| {
        panel.sync(None);
        !panel.file_search_pending() && contains(&panel, &matched)
    });
    let generation = panel.search_generation;
    let epoch = panel.search_index_epoch;
    let unrelated = directory.path().join("unrelated.txt");
    std::fs::write(&unrelated, "").unwrap();
    wait_until(|| {
        panel.sync(None);
        panel.tree_rows.iter().any(|row| row.path == unrelated)
    });
    assert!(!contains(&panel, &unrelated));
    assert_eq!((panel.search_generation, panel.search_index_epoch), (generation, epoch));
    panel.set_file_search_query(String::new());
    assert!(contains(&panel, &unrelated));
    panel.file_index.release_for_test();
}

#[test]
fn stale_snapshot_cannot_undo_an_expansion_or_an_aba_root_switch() {
    let directory = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let folder = directory.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("child"), "").unwrap();
    let mut panel = open(directory.path());
    let old_revision = panel.snapshot_revision;
    let old_tree_revision = panel.tree_revision;
    let old_rows = panel.tree_rows.clone();
    panel.snapshot_running.store(true, Ordering::Release);
    let index = panel.file_rows().iter().position(|row| row.path == folder).unwrap();
    assert!(panel.click_row(index));
    let snapshot = || PanelSnapshot {
        revision: old_revision,
        tree_revision: old_tree_revision,
        root: directory.path().to_owned(),
        files_wsl: None,
        rows: old_rows.clone(),
        enumeration_ok: true,
        git: None,
    };
    *panel.snapshot_slot.lock().unwrap() = Some(snapshot());
    assert!(!panel.harvest_snapshot());
    assert!(contains(&panel, &folder.join("child")));
    panel.snapshot_running.store(false, Ordering::Release);
    panel.sync(Some(other.path().to_owned()));
    panel.sync(Some(directory.path().to_owned()));
    *panel.snapshot_slot.lock().unwrap() = Some(snapshot());
    assert!(!panel.harvest_snapshot());
    settle(&mut panel);
}

#[test]
fn expansion_keeps_the_inflight_vcs_result_without_restoring_old_rows() {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("child"), "").unwrap();
    let mut panel = open(directory.path());
    let revision = panel.snapshot_revision;
    let tree_revision = panel.tree_revision;
    let rows = panel.tree_rows.clone();
    // Hold the original full snapshot in flight while the user expands a row.
    panel.snapshot_running.store(true, Ordering::Release);
    let index = panel.file_rows().iter().position(|row| row.path == folder).unwrap();
    assert!(panel.click_row(index));
    *panel.snapshot_slot.lock().unwrap() = Some(PanelSnapshot {
        revision,
        tree_revision,
        root: directory.path().to_owned(),
        files_wsl: None,
        rows,
        enumeration_ok: true,
        git: Some(Some(GitInfo { branch: "finished-vcs".into(), ..Default::default() })),
    });
    assert!(panel.harvest_snapshot());
    panel.snapshot_running.store(false, Ordering::Release);
    assert!(contains(&panel, &folder.join("child")));
    assert_eq!(panel.git().map(|git| git.branch.as_str()), Some("finished-vcs"));
}

#[test]
fn event_during_a_snapshot_remains_pending_and_idle_sync_does_no_work() {
    let directory = tempfile::tempdir().unwrap();
    let mut panel = open(directory.path());
    let revision = panel.snapshot_revision;
    for _ in 0..100 {
        panel.sync(None);
    }
    assert_eq!(panel.snapshot_revision, revision);
    panel.snapshot_running.store(true, Ordering::Release);
    let file = directory.path().join("during-snapshot.txt");
    std::fs::write(&file, "").unwrap();
    wait_until(|| panel.tree_watch.signal.dirty.load(Ordering::Acquire));
    panel.sync(None);
    assert!(panel.tree_watch.signal.dirty.load(Ordering::Acquire));
    panel.snapshot_running.store(false, Ordering::Release);
    expect_path(&mut panel, &file, true);
}

#[test]
fn closing_changing_views_and_drop_release_the_worker() {
    let directory = tempfile::tempdir().unwrap();
    let mut panel = open(directory.path());
    let closed = Arc::downgrade(&panel.tree_watch.worker.as_ref().unwrap().0);
    panel.toggle(PanelView::Files);
    assert!(!panel.tree_watch.active());
    wait_until(|| closed.upgrade().is_none());
    panel.toggle(PanelView::Files);
    panel.sync(None);
    settle(&mut panel);
    let git = Arc::downgrade(&panel.tree_watch.worker.as_ref().unwrap().0);
    panel.toggle(PanelView::Git);
    wait_until(|| git.upgrade().is_none());
    panel.toggle(PanelView::Files);
    panel.sync(None);
    settle(&mut panel);
    let dropped = Arc::downgrade(&panel.tree_watch.worker.as_ref().unwrap().0);
    drop(panel);
    wait_until(|| dropped.upgrade().is_none());
}

#[test]
fn remote_suspension_resumes_locally_but_wsl_never_arms_host_paths() {
    let directory = tempfile::tempdir().unwrap();
    let mut panel = open(directory.path());
    panel.suspend_file_watching();
    assert!(!panel.tree_watch.active());
    panel.sync(None);
    settle(&mut panel);
    assert!(panel.tree_watch.active());
    panel.followed_cwd = None;
    panel.followed_wsl = Some(crate::shell_detect::WslCwd {
        distro: "not-a-host-path".into(),
        guest: "/home".into(),
    });
    panel.sync_tree_watch();
    assert!(!panel.tree_watch.active());
}

#[test]
fn bursts_are_bounded_without_losing_the_trailing_change() {
    let mut watch = TreeWatch::default();
    let start = Instant::now();
    for _ in 0..10_000 {
        watch.signal.dirty.store(true, Ordering::Release);
    }
    assert!(watch.take_changed(start));
    assert!(!watch.take_changed(start + REFRESH_INTERVAL));
    watch.signal.dirty.store(true, Ordering::Release);
    assert!(!watch.take_changed(start + Duration::from_millis(1)));
    assert!(watch.signal.dirty.load(Ordering::Acquire));
    assert!(watch.take_changed(start + REFRESH_INTERVAL));
    assert!(!watch.take_changed(start + REFRESH_INTERVAL * 2));
}

#[test]
fn event_filter_ignores_reads_file_writes_git_internals_and_siblings() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let scope = WatchScope::new(&root, &[]);
    let event = |kind, path| Event::new(kind).add_path(path);
    assert!(
        !scope.invalidates(&event(
            EventKind::Access(notify::event::AccessKind::Any),
            root.join("file")
        ))
    );
    assert!(!scope.invalidates(&event(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        root.join("file")
    )));
    assert!(!scope.invalidates(&event(EventKind::Create(CreateKind::Any), root.join(".git"))));
    assert!(
        !scope.invalidates(&event(
            EventKind::Create(CreateKind::Any),
            directory.path().join("sibling")
        ))
    );
    assert!(scope.invalidates(&event(EventKind::Create(CreateKind::Any), root.join("project"))));
    assert!(scope.invalidates(&event(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        root.join(".gitignore")
    )));
    assert!(scope.invalidates(&Event::new(EventKind::Any)));
}

#[test]
fn errors_overflow_and_directory_replacement_request_rearming() {
    let scope = WatchScope::new(Path::new("root"), &[]);
    for event in [
        Err(notify::Error::generic("watch failed")),
        Ok(Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan)),
        Ok(Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(scope.root.clone())),
        Ok(Event::new(EventKind::Create(CreateKind::Folder)).add_path(scope.root.clone())),
        Ok(Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(scope.root.clone())),
    ] {
        let signal = Signal::default();
        signal.record(&scope, event);
        assert!(signal.dirty.load(Ordering::Acquire));
        assert!(signal.rearm.load(Ordering::Acquire));
    }
}

#[test]
fn watch_scopes_are_bounded_and_old_callbacks_cannot_dirty_new_roots() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let rows: Vec<_> = (0..2000)
        .map(|i| FileRow {
            path: root.join(format!("dir{i}")),
            guest_path: None,
            name: format!("dir{i}"),
            depth: 0,
            is_dir: true,
            expanded: true,
            is_parent: false,
            ignored: false,
        })
        .collect();
    let scope = WatchScope::new(root, &rows);
    assert!(scope.directories.len() < MAX_WATCHES);
    assert!(
        scope.directories.iter().map(|path| path.as_os_str().len()).sum::<usize>()
            <= MAX_PATH_BYTES
    );
    let mut watch = TreeWatch::default();
    watch.configure(Some(WatchScope::new(root, &[])));
    wait_until(|| watch.signal.dirty.load(Ordering::Acquire));
    let old = watch.signal.clone();
    let other = tempfile::tempdir().unwrap();
    watch.configure(Some(WatchScope::new(other.path(), &[])));
    wait_until(|| watch.signal.dirty.load(Ordering::Acquire));
    watch.signal.dirty.store(false, Ordering::Release);
    old.dirty.store(true, Ordering::Release);
    assert!(!watch.take_changed(Instant::now()));
}
