//! 选择性恢复及其加密恢复点；UI 只持有句柄，不负责文件写入规则。
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct RestorePoint {
    pub path: PathBuf,
    names: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct OriginalFile {
    name: String,
    bytes: Option<Vec<u8>>,
}

fn read_original(root: &Path, name: &str) -> Result<OriginalFile, String> {
    let path = restore_path(root, name)?;
    if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(format!("backup destination is a symbolic link: {name}"));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("read recovery source {name}: {error}")),
    };
    Ok(OriginalFile { name: name.to_owned(), bytes })
}

fn write_originals(root: &Path, originals: &[OriginalFile]) -> Result<(), String> {
    let paths = originals
        .iter()
        .map(|file| restore_path(root, &file.name))
        .collect::<Result<Vec<_>, _>>()?;
    for (original, path) in originals.iter().zip(paths) {
        match &original.bytes {
            Some(bytes) => crate::atomic_file::write(&path, bytes)
                .map_err(|error| format!("undo {}: {error}", original.name))?,
            None => match fs::remove_file(path) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(format!("undo {}: {error}", original.name)),
            },
        }
    }
    Ok(())
}

pub(crate) fn restore_selected(
    archive: &BackupArchive,
    selection: BackupSelection,
    passphrase: &str,
) -> Result<RestorePoint, String> {
    restore_selected_at(crate::platform::dirs::data_dir(), archive, selection, passphrase)
}

fn restore_selected_at(
    root: &Path,
    archive: &BackupArchive,
    selection: BackupSelection,
    passphrase: &str,
) -> Result<RestorePoint, String> {
    validate_archive(archive)?;
    let categories: HashSet<_> = selection.categories().collect();
    if categories.is_empty() {
        return Err("Select at least one category to restore".into());
    }
    let mut selected = archive.clone();
    selected.entries.retain(|entry| categories.contains(&entry.category));
    selected.manifest.categories.retain(|category| categories.contains(category));
    let originals = selected
        .entries
        .iter()
        .map(|entry| read_original(root, &entry.name))
        .collect::<Result<Vec<_>, _>>()?;
    // 精确保存原字节和“不存在”状态：撤回不能丢掉本机 SSH 偏好或留下恢复新建的文件。
    // 恢复点只在本机加密落盘，不经过会过滤凭据字段的可分享备份收集器。
    let plaintext = zeroize::Zeroizing::new(
        serde_json::to_vec(&originals)
            .map_err(|error| format!("serialize recovery point: {error}"))?,
    );
    let packet = encrypt_bytes(&plaintext, passphrase)?;
    let name = format!(
        "backup-recovery/{}.pebrel-recovery",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%f")
    );
    let path = restore_path(root, &name)?;
    fs::create_dir_all(path.parent().ok_or("invalid recovery path")?)
        .map_err(|error| format!("create recovery directory: {error}"))?;
    crate::atomic_file::write(&path, &packet)
        .map_err(|error| format!("save recovery point: {error}"))?;
    if let Err(error) = restore_to(root, &selected) {
        return match write_originals(root, &originals) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; {rollback}; recovery: {}", path.display())),
        };
    }
    Ok(RestorePoint { path, names: originals.into_iter().map(|file| file.name).collect() })
}

pub(crate) fn undo(point: &RestorePoint, passphrase: &str) -> Result<(), String> {
    undo_at(crate::platform::dirs::data_dir(), point, passphrase)
}

fn undo_at(root: &Path, point: &RestorePoint, passphrase: &str) -> Result<(), String> {
    let packet = fs::read(&point.path).map_err(|error| format!("read recovery point: {error}"))?;
    let plaintext = decrypt_bytes(&packet, passphrase)?;
    let originals: Vec<OriginalFile> = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("invalid recovery point: {error}"))?;
    // 只接受该次已验证恢复操作拥有的目标集合，外部导入文件不能借撤回扩展白名单。
    if originals.iter().map(|file| &file.name).ne(point.names.iter()) {
        return Err("recovery target mismatch".into());
    }
    write_originals(root, &originals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selective_restore_preserves_other_categories_and_undo_restores_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("pebrel_settings.txt"), b"theme=old\npinned_hosts=local")
            .unwrap();
        let archive = BackupArchive {
            manifest: BackupManifest {
                version: 1,
                device: String::new(),
                categories: vec![BackupCategory::Appearance, BackupCategory::Session],
            },
            entries: vec![
                BackupEntry {
                    category: BackupCategory::Appearance,
                    name: "pebrel_settings.txt".into(),
                    bytes: b"theme=new".to_vec(),
                },
                BackupEntry {
                    category: BackupCategory::Session,
                    name: "session.json".into(),
                    bytes: b"{}".to_vec(),
                },
            ],
        };
        let point =
            restore_selected_at(root.path(), &archive, BackupSelection::default(), "correct horse")
                .unwrap();
        assert_eq!(fs::read(root.path().join("pebrel_settings.txt")).unwrap(), b"theme=new");
        assert!(!root.path().join("session.json").exists());
        assert!(undo_at(root.path(), &point, "wrong horse").is_err());
        undo_at(root.path(), &point, "correct horse").unwrap();
        assert_eq!(
            fs::read(root.path().join("pebrel_settings.txt")).unwrap(),
            b"theme=old\npinned_hosts=local"
        );
        let point = restore_selected_at(
            root.path(),
            &archive,
            BackupSelection::from_categories([BackupCategory::Session]),
            "correct horse",
        )
        .unwrap();
        assert!(root.path().join("session.json").exists());
        undo_at(root.path(), &point, "correct horse").unwrap();
        assert!(!root.path().join("session.json").exists());
    }
}
