use super::*;

pub(super) fn wait_for_result(
    index: &EmbeddedFileIndex,
    matches: impl Fn(&FileSearchResult) -> bool,
) -> FileSearchResult {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(result) = index.take_result()
            && matches(&result)
            && result.complete
        {
            return result;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("file search did not publish the expected result");
}

fn start(root: &Path, query: &str) -> EmbeddedFileIndex {
    let index = EmbeddedFileIndex::new();
    index.rebuild(
        Some(FileIndexRoot::Local(root.to_owned())),
        1,
        Some((1, query.to_owned(), FileSearchOptions::default())),
    );
    index
}

fn scan(root: &Path) -> Vec<IndexedPath> {
    let mut entries = Vec::new();
    let result = walk::local(root, || false, |_| {}, |entry| entries.push(entry));
    assert!(result.error.is_none());
    assert!(!result.limited);
    entries
}

#[test]
fn local_index_crawls_nested_paths_but_hides_git_objects() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/nested")).unwrap();
    std::fs::create_dir_all(temp.path().join(".git/objects")).unwrap();
    std::fs::write(temp.path().join("src/nested/needle.rs"), b"").unwrap();
    std::fs::write(temp.path().join(".git/objects/hidden"), b"").unwrap();
    let entries = scan(temp.path());
    assert!(entries.iter().any(|entry| entry.key == "src/nested/needle.rs"));
    assert!(!entries.iter().any(|entry| entry.key.contains(".git")));
}

#[test]
fn watched_index_tracks_create_rename_delete_without_changing_root() {
    let temp = tempfile::tempdir().unwrap();
    let index = start(temp.path(), "needle");
    wait_for_result(&index, |result| result.total == 0);
    let created = temp.path().join("needle.txt");
    std::fs::write(&created, b"").unwrap();
    wait_for_result(&index, |result| result.total == 1);
    let renamed = temp.path().join("needle-renamed.txt");
    std::fs::rename(&created, &renamed).unwrap();
    wait_for_result(&index, |result| result.total == 1 && result.rows[0].path == renamed);
    std::fs::remove_file(&renamed).unwrap();
    let result = wait_for_result(&index, |result| result.total == 0);
    assert_eq!(result.epoch, 1);
    index.release_for_test();
}

#[test]
fn large_change_batches_preserve_unrelated_index_entries() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("needle-untouched.txt"), b"").unwrap();
    let index = start(temp.path(), "needle");
    wait_for_result(&index, |result| result.total == 1);
    for number in 0..128 {
        std::fs::write(temp.path().join(format!("needle-changed-{number}.txt")), b"").unwrap();
    }
    let result = wait_for_result(&index, |result| result.total == 129);
    assert!(result.rows.iter().any(|row| row.name == "needle-untouched.txt"));
    index.release_for_test();
}

#[test]
fn refreshed_index_deduplicates_overlapping_paths_and_hides_git_events() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("src/nested/needle.rs");
    let hidden = temp.path().join(".git/objects/needle");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(hidden.parent().unwrap()).unwrap();
    std::fs::write(&source, b"").unwrap();
    std::fs::write(&hidden, b"").unwrap();
    let index = start(temp.path(), "needle");
    wait_for_result(&index, |result| result.total == 1);
    for _ in 0..8 {
        index.refresh(1);
    }
    let result = wait_for_result(&index, |result| result.total == 1);
    assert_eq!(result.rows[0].path, source);
    index.release_for_test();
}

#[test]
fn cancelled_search_cannot_publish_after_newer_query_or_clear() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("newest.txt"), b"").unwrap();
    let index = start(temp.path(), "old");
    for generation in 2..100 {
        index.query(1, generation, "old".to_owned(), FileSearchOptions::default());
    }
    index.query(1, 100, "newest".to_owned(), FileSearchOptions::default());
    let result = wait_for_result(&index, |result| result.generation == 100);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(index.scans(), 1, "superseded debounce requests must not start walks");
    index.clear_query();
    std::thread::sleep(Duration::from_millis(250));
    assert!(index.take_result().is_none());
    assert_eq!(index.status(), FileIndexStatus::Idle);
    index.release_for_test();
}

#[test]
fn empty_search_does_no_index_work_and_repeated_queries_reuse_cache() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("needle.rs"), b"").unwrap();
    let index = EmbeddedFileIndex::new();
    index.rebuild(Some(FileIndexRoot::Local(temp.path().to_owned())), 1, None);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(index.scans(), 0);
    assert_eq!(index.retained_bytes(), 0);
    index.query(1, 1, "needle".to_owned(), FileSearchOptions::default());
    wait_for_result(&index, |result| result.total == 1);
    let bytes = index.retained_bytes();
    for generation in 2..8 {
        index.clear_query();
        index.query(1, generation, "needle".to_owned(), FileSearchOptions::default());
        wait_for_result(&index, |result| result.generation == generation && result.total == 1);
        assert_eq!(index.retained_bytes(), bytes);
    }
    assert_eq!(index.scans(), 1);
    index.release_for_test();
    assert_eq!(index.retained_bytes(), 0);
}

#[test]
fn root_switch_replaces_owned_cache_and_returns_the_new_roots_paths() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("needle-a.txt"), b"").unwrap();
    std::fs::write(second.path().join("needle-b.txt"), b"").unwrap();
    let index = start(first.path(), "needle");
    wait_for_result(&index, |result| result.rows[0].name == "needle-a.txt");
    for epoch in 2..10 {
        let root = if epoch % 2 == 0 { second.path() } else { first.path() };
        index.rebuild(
            Some(FileIndexRoot::Local(root.to_owned())),
            epoch,
            Some((epoch, "needle".to_owned(), FileSearchOptions::default())),
        );
        let result = wait_for_result(&index, |result| result.epoch == epoch);
        assert_eq!(result.rows.len(), 1);
        assert!(result.rows[0].path.starts_with(root));
        assert!(index.retained_bytes() < 64 * 1024);
    }
    index.release_for_test();
}

#[test]
fn repeated_cache_prefix_visits_do_not_append_or_reallocate() {
    let mut cache = cache::FileCache::new();
    let entries: Vec<_> = (0..100).map(|n| entry(&format!("src/{n}.rs"))).collect();
    for (position, entry) in entries.iter().enumerate() {
        cache.record(position, entry);
    }
    let bytes = cache.memory.bytes();
    let address = cache.entries.as_ptr();
    for _ in 0..100 {
        for (position, entry) in entries.iter().enumerate() {
            cache.record(position, entry);
        }
    }
    assert_eq!(cache.memory.bytes(), bytes);
    assert_eq!(cache.entries.as_ptr(), address);
    assert_eq!(cache.entries.len(), entries.len());
}

#[test]
fn cache_limit_counts_strings_and_array_capacity_not_only_record_count() {
    let mut cache = cache::FileCache::new();
    for position in 0..50_000 {
        cache.record(position, &entry(&format!("{}/{position}.rs", "long-path/".repeat(64))));
    }
    assert!(cache.full);
    assert!(cache.entries.len() < 50_000);
    let actual = cache.entries.capacity() * std::mem::size_of::<IndexedPath>()
        + cache.entries.iter().map(IndexedPath::heap_bytes).sum::<usize>();
    assert!(actual <= cache.memory.bytes());
    assert!(cache.memory.bytes() <= memory::CACHE_LIMIT);
}

#[test]
fn streamed_ranking_keeps_a_late_exact_match_and_bounds_matching_storage() {
    let request = SearchRequest {
        epoch: 1,
        generation: 1,
        query: "needle".to_owned(),
        options: FileSearchOptions::default(),
    };
    let mut best = BestMatches::new(&request);
    for n in 0..20_000 {
        best.add(entry(&format!("src/needle-{n:06}.rs")));
    }
    best.add(entry("deep/needle"));
    let (result, _) = best.result(&request);
    assert_eq!(result.total, 20_001);
    assert_eq!(result.rows.len(), MAX_SEARCH_RESULTS);
    assert_eq!(result.rows[0].name, "needle");
    assert!(best.limited);
    assert!(result.memory.bytes() <= memory::RESULT_LIMIT);
}

#[test]
fn unreadable_root_is_reported_instead_of_successful_empty_results() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("gone");
    let index = start(&missing, "needle");
    let result = wait_for_result(&index, |result| result.error.is_some());
    assert!(result.rows.is_empty());
    index.release_for_test();
}

#[test]
fn walk_obeys_cancellation_before_reading_descendants() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("nested/deep")).unwrap();
    let mut calls = 0;
    let result = walk::local(temp.path(), || true, |_| {}, |_| calls += 1);
    assert_eq!(calls, 0);
    assert_eq!(result.visited, 0);
}

fn entry(key: &str) -> IndexedPath {
    let name = key.rsplit('/').next().unwrap_or(key).to_owned();
    IndexedPath {
        path: PathBuf::from(key),
        guest_path: None,
        name_folded: name.to_lowercase(),
        key_folded: key.to_lowercase(),
        name,
        key: key.to_owned(),
        is_dir: false,
    }
}

#[test]
fn plain_search_is_case_insensitive_and_ands_terms() {
    let entries = [entry("src/FileTree/SearchIndex.rs"), entry("docs/search.md")];
    let request = SearchRequest {
        epoch: 1,
        generation: 1,
        query: "TREE index".to_owned(),
        options: FileSearchOptions::default(),
    };
    let result = search_entries(&entries, &request);
    assert_eq!(result.total, 1);
    assert_eq!(result.rows[0].name, "SearchIndex.rs");
}

#[test]
fn case_whole_word_and_regex_options_share_one_matcher() {
    let entries = [entry("src/Foo.rs"), entry("src/foo_bar.rs"), entry("src/food.rs")];
    let whole_word = SearchRequest {
        epoch: 1,
        generation: 1,
        query: "foo".to_owned(),
        options: FileSearchOptions { whole_word: true, ..Default::default() },
    };
    assert_eq!(search_entries(&entries, &whole_word).total, 1);

    let case_sensitive = SearchRequest {
        epoch: 1,
        generation: 2,
        query: "Foo".to_owned(),
        options: FileSearchOptions { match_case: true, ..Default::default() },
    };
    assert_eq!(search_entries(&entries, &case_sensitive).total, 1);

    let regex = SearchRequest {
        epoch: 1,
        generation: 3,
        query: r"foo(?:d|_bar)".to_owned(),
        options: FileSearchOptions { regex: true, ..Default::default() },
    };
    assert_eq!(search_entries(&entries, &regex).total, 2);
}

#[test]
fn invalid_regex_is_reported_without_stale_rows() {
    let result = search_entries(
        &[entry("src/main.rs")],
        &SearchRequest {
            epoch: 1,
            generation: 7,
            query: "[".to_owned(),
            options: FileSearchOptions { regex: true, ..Default::default() },
        },
    );
    assert_eq!(result.generation, 7);
    assert!(result.rows.is_empty());
    assert!(result.error.is_some());
}

#[test]
fn nested_watch_invalidates_a_warm_cache() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let index = start(temp.path(), "needle");
    wait_for_result(&index, |result| result.total == 0);
    std::fs::write(nested.join("needle.txt"), b"").unwrap();
    wait_for_result(&index, |result| result.total == 1);
    index.release_for_test();
}

#[test]
fn published_rows_keep_their_memory_lease_after_worker_clear() {
    let temp = tempfile::tempdir().unwrap();
    for number in 0..100 {
        std::fs::write(temp.path().join(format!("needle-{number}.txt")), b"").unwrap();
    }
    let index = start(temp.path(), "needle");
    let result = wait_for_result(&index, |result| result.total == 100);
    let actual = result.rows.capacity() * std::mem::size_of::<FileRow>()
        + result
            .rows
            .iter()
            .map(|row| {
                row.path.capacity()
                    + row.name.capacity()
                    + row.guest_path.as_ref().map_or(0, String::capacity)
            })
            .sum::<usize>();
    assert!(actual <= result.memory.bytes());
    index.release_for_test();
    assert_eq!(index.retained_bytes(), 0);
    assert!(result.memory.bytes() >= actual, "view-owned rows still consume their quota");
}

#[test]
fn shorter_fresh_walk_drops_stale_cached_tail() {
    let root = Path::new("fixture");
    let mut cache = cache::FileCache::new();
    for position in 0..3 {
        let entry = indexed_local_path(root, &root.join(format!("{position}.txt")), false).unwrap();
        cache.record(position, &entry);
    }
    let before = cache.memory.bytes();
    let address = cache.entries.as_ptr();
    cache.finish_prefix(1);
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.memory.bytes() < before);
    assert_eq!(cache.entries.as_ptr(), address);
}
