//! Immutable, content-versioned adapters in the application's own runtime directory.
//! No agent's global configuration is installed or rewritten here.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest as _, Sha256};

const ASSETS: &[(&str, &str)] = &[
    ("claude/package.json", include_str!("../../res/prompt-suggestions/claude/package.json")),
    (
        "claude/.claude-plugin/plugin.json",
        include_str!("../../res/prompt-suggestions/claude/.claude-plugin/plugin.json"),
    ),
    (
        "claude/hooks/hooks.json",
        include_str!("../../res/prompt-suggestions/claude/hooks/hooks.json"),
    ),
    ("claude/hooks/prompt.js", include_str!("../../res/prompt-suggestions/claude/hooks/prompt.js")),
    ("pi/index.ts", include_str!("../../res/prompt-suggestions/pi/index.ts")),
    ("pi/editor.mjs", include_str!("../../res/prompt-suggestions/pi/editor.mjs")),
    ("opencode/index.tsx", include_str!("../../res/prompt-suggestions/opencode/index.tsx")),
    ("opencode/session.mjs", include_str!("../../res/prompt-suggestions/opencode/session.mjs")),
    // OpenCode resolves path-like specs relative to the declaring config file.
    ("opencode/tui.json", "{\"plugin\":[\"./index.tsx\"]}\n"),
    ("shared/client.mjs", include_str!("../../res/prompt-suggestions/shared/client.mjs")),
];

/// Called by the thin launcher, never by a view/render callback. Once per process;
/// an already installed version is verified without rewriting any file.
pub(crate) fn ensure() -> Result<PathBuf, String> {
    static INSTALLED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            let directory = crate::platform::dirs::data_dir().join("runtime/prompt-suggestions");
            install_at(&directory, ASSETS).map_err(|error| error.to_string())
        })
        .clone()
}

fn version(assets: &[(&str, &str)]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"pebrel-native-predictions-bundle-v2");
    for (name, content) in assets {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((content.len() as u64).to_le_bytes());
        hash.update(content.as_bytes());
    }
    hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn conflict(path: &Path) -> io::Error {
    io::Error::other(format!(
        "preserving edited or incomplete prediction adapter: {}",
        path.display()
    ))
}

fn verify(root: &Path, assets: &[(&str, &str)]) -> io::Result<()> {
    if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
        return Err(conflict(root));
    }
    for (name, content) in assets {
        let mut path = root.to_path_buf();
        let mut components = Path::new(name).components().peekable();
        while let Some(component) = components.next() {
            path.push(component);
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| conflict(&path))?;
            if components.peek().is_some() {
                if !metadata.file_type().is_dir() {
                    return Err(conflict(&path));
                }
            } else if !metadata.file_type().is_file()
                || metadata.len() != content.len() as u64
                || std::fs::read(&path)? != content.as_bytes()
            {
                return Err(conflict(&path));
            }
        }
    }
    Ok(())
}

fn install_at(directory: &Path, assets: &[(&str, &str)]) -> io::Result<PathBuf> {
    let target = directory.join(version(assets));
    match std::fs::symlink_metadata(&target) {
        Ok(_) => {
            verify(&target, assets)?;
            return Ok(target);
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }

    std::fs::create_dir_all(directory)?;
    let staging = tempfile::Builder::new().prefix(".install-").tempdir_in(directory)?;
    for (name, content) in assets {
        let path = staging.path().join(name);
        std::fs::create_dir_all(path.parent().expect("static asset path has a parent"))?;
        std::fs::write(path, content)?;
    }
    // Publish the complete derived bundle at once. Concurrent launchers either
    // install the same version or verify the winner; no partially written code
    // is handed to an agent. Old versions and manual edits are left untouched.
    if let Err(error) = std::fs::rename(staging.path(), &target) {
        if std::fs::symlink_metadata(&target).is_err() {
            return Err(error);
        }
        verify(&target, assets)?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_complete_bundle_once_without_rewriting_and_keeps_old_versions() {
        let directory = tempfile::tempdir().unwrap();
        let assets = [("claude/hooks/prompt.js", "export const version = 1;")];
        let first = install_at(directory.path(), &assets).unwrap();
        let metadata = std::fs::metadata(first.join(assets[0].0)).unwrap();
        assert_eq!(install_at(directory.path(), &assets).unwrap(), first);
        assert_eq!(
            std::fs::metadata(first.join(assets[0].0)).unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
        let second = install_at(directory.path(), &[(assets[0].0, "version two")]).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(first.join(assets[0].0)).unwrap(), assets[0].1);
        let bundle = install_at(directory.path(), ASSETS).unwrap();
        let tui: serde_json::Value =
            serde_json::from_slice(&std::fs::read(bundle.join("opencode/tui.json")).unwrap())
                .unwrap();
        assert_eq!(tui["plugin"][0], "./index.tsx");
        assert!(bundle.join("opencode/index.tsx").is_file());
    }

    #[test]
    fn edited_or_missing_files_are_preserved_without_partial_repair() {
        let directory = tempfile::tempdir().unwrap();
        let assets = [("pi/index.ts", "managed"), ("shared/client.mjs", "shared")];
        let root = install_at(directory.path(), &assets).unwrap();
        std::fs::write(root.join("pi/index.ts"), "user customization").unwrap();
        assert!(install_at(directory.path(), &assets).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("pi/index.ts")).unwrap(),
            "user customization"
        );
        std::fs::remove_file(root.join("shared/client.mjs")).unwrap();
        assert!(install_at(directory.path(), &assets).is_err());
        assert!(!root.join("shared/client.mjs").exists());
    }

    #[test]
    fn concurrent_installers_publish_one_whole_version() {
        let directory = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        install_at(directory.path(), &[("pi/index.ts", "managed")]).unwrap()
                    })
                })
                .collect();
            let roots: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
            assert!(roots.iter().all(|root| *root == roots[0]));
            assert_eq!(std::fs::read_to_string(roots[0].join("pi/index.ts")).unwrap(), "managed");
        });
    }

    #[test]
    fn existing_symlink_is_never_followed_or_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let assets = [("pi/index.ts", "managed")];
        let root = directory.path().join(version(&assets));
        // Preserve the Unix regression on hosts where directory symlinks are
        // available. Windows may require a privilege absent from a test token.
        if let Err(error) =
            crate::platform::local_paths::symlink_directory_for_test(other.path(), &root)
        {
            if crate::platform::Platform::current() == crate::platform::Platform::Windows
                && error.raw_os_error() == Some(1314)
            {
                eprintln!("directory symlink regression not run: {error}");
                return;
            }
            panic!("directory symlink fixture: {error}");
        }
        assert!(install_at(directory.path(), &assets).is_err());
        assert!(root.is_symlink());
        assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
    }
}
