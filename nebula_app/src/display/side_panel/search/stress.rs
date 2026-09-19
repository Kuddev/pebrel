//! Explicit, slower filesystem/allocator exercise using the production worker.
//! Only owned temporary fixtures are created, searched and removed.

use super::*;

fn answer(panel: &mut SidePanel, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        panel.harvest_snapshot();
        panel.harvest_file_search();
        if !panel.file_search_pending() && panel.file_search_total() == expected {
            assert!(panel.file_search_error().is_none());
            assert_eq!(panel.file_rows().first().map(|row| row.name.as_str()), Some("needle"));
            return;
        }
        assert!(Instant::now() < deadline, "search stress timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn row_bytes(rows: &Vec<FileRow>) -> usize {
    rows.capacity() * std::mem::size_of::<FileRow>()
        + rows
            .iter()
            .map(|row| {
                row.path.capacity()
                    + row.name.capacity()
                    + row.guest_path.as_ref().map_or(0, String::capacity)
            })
            .sum::<usize>()
}

#[cfg(windows)]
fn process_memory(array: usize) -> serde_json::Value {
    #[repr(C)]
    struct Counters {
        size: u32,
        faults: u32,
        values: [usize; 9],
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: isize, counters: *mut Counters, size: u32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessHeap() -> isize;
        fn HeapSize(heap: isize, flags: u32, memory: *const std::ffi::c_void) -> usize;
    }
    let mut counters =
        Counters { size: std::mem::size_of::<Counters>() as u32, faults: 0, values: [0; 9] };
    let ok =
        unsafe { GetProcessMemoryInfo(-1, &mut counters, std::mem::size_of::<Counters>() as u32) };
    assert_ne!(ok, 0);
    let block = if array > 4096 {
        let bytes = unsafe { HeapSize(GetProcessHeap(), 0, array as *const _) };
        (bytes != usize::MAX).then_some(bytes)
    } else {
        None
    };
    serde_json::json!({"private_commit": counters.values[8], "working_set": counters.values[1], "array_heap_block_bytes": block})
}

#[cfg(not(windows))]
fn process_memory(_: usize) -> serde_json::Value {
    serde_json::Value::Null
}

#[test]
#[ignore = "Creates 27,200 files and exercises repeated scans, root switches and allocator retention"]
fn file_search_memory_stress() {
    let fixture = tempfile::tempdir().unwrap();
    let mut roots = Vec::new();
    for (folder, files) in [("wide", 16_000), ("medium", 6_000), ("small", 2_000), ("nested", 160)]
    {
        let root = fixture.path().join(folder);
        std::fs::create_dir(&root).unwrap();
        for number in 0..files {
            if folder == "nested" {
                let directory = root.join(format!("directory-{number:04}"));
                std::fs::create_dir(&directory).unwrap();
                for child in 0..20 {
                    std::fs::write(directory.join(format!("item-{child:02}-needle.txt")), b"")
                        .unwrap();
                }
                continue;
            }
            let name = format!("item-{number:05}-{}-needle.txt", "abcdefghij".repeat(7));
            std::fs::write(root.join(name), b"").unwrap();
        }
        std::fs::write(root.join("needle"), b"").unwrap();
        roots.push(root);
    }
    let baseline = process_memory(0);
    let mut panel = SidePanel::new();
    panel.toggle(PanelView::Files);
    let mut samples = Vec::new();

    for cycle in 0..30 {
        for (root_number, root) in roots.iter().enumerate() {
            let started = Instant::now();
            panel.set_custom_root(root.clone());
            panel.wait_snapshot();
            panel.set_file_search_query("needle".to_owned());
            let expected = [16_001, 6_001, 2_001, 3_201][root_number];
            answer(&mut panel, expected);
            let allocation = panel.file_index.allocation_snapshot();
            assert!(allocation.array_bytes + allocation.string_bytes <= memory::CACHE_LIMIT);
            let scans = panel.file_index.scans();
            let address = allocation.array_address;
            let elapsed = started.elapsed().as_millis();
            for repetition in 0..3 {
                panel.set_file_search_query(String::new());
                assert!(panel.search_memory.is_none());
                panel.set_file_search_query("needle".to_owned());
                answer(&mut panel, expected);
                panel.toggle(PanelView::Files);
                assert_eq!(panel.rows.capacity(), 0);
                assert!(panel.search_memory.is_none());
                panel.toggle(PanelView::Files);
                answer(&mut panel, expected);
                assert_eq!(
                    panel.file_index.allocation_snapshot().array_address,
                    address,
                    "cache reallocated during reuse"
                );
                if root_number == 2 {
                    assert_eq!(
                        panel.file_index.scans(),
                        scans,
                        "small directory rebuilt during reuse"
                    );
                }
                let allocation = panel.file_index.allocation_snapshot();
                samples.push(serde_json::json!({
                    "cycle":cycle,"root":root_number,"repetition":repetition,
                    "first_search_ms":elapsed,"cache":allocation,
                    "memory":process_memory(allocation.array_address),"scans":panel.file_index.scans(),
                    "tree_bytes":row_bytes(&panel.tree_rows),"result_bytes":row_bytes(&panel.rows),
                }));
            }
        }
        println!("FILE_SEARCH_MEMORY_CYCLE {cycle}");
    }
    panel.toggle(PanelView::Files);
    panel.file_index.release_for_test();
    assert_eq!(panel.file_index.allocation_snapshot().array_bytes, 0);
    assert_eq!(panel.file_index.allocation_snapshot().string_bytes, 0);
    let final_memory = process_memory(0);
    let evidence = serde_json::json!({"pid":std::process::id(),"baseline":baseline,"samples":samples,"after_release":final_memory});
    println!("FILE_SEARCH_MEMORY_STRESS {}", evidence);
    // Compare the same root after allocator warmup. Residency can be trimmed by
    // the OS, so the Windows stability check uses private commit, not working set.
    if let (Some(first), Some(last)) = (
        samples
            .iter()
            .find(|row| row["cycle"] == 25 && row["root"] == 0)
            .and_then(|row| row["memory"]["private_commit"].as_u64()),
        samples
            .iter()
            .rev()
            .find(|row| row["root"] == 0)
            .and_then(|row| row["memory"]["private_commit"].as_u64()),
    ) {
        assert!(last <= first + 2 * 1024 * 1024, "private commit kept growing: {first} -> {last}");
        let baseline = evidence["baseline"]["private_commit"].as_u64().unwrap();
        for row in &samples {
            let used = row["memory"]["private_commit"].as_u64().unwrap();
            assert!(
                used <= baseline + 15 * 1024 * 1024,
                "sustained browsing/search overhead exceeded 15 MiB: {baseline} -> {used}"
            );
        }
    }
}
