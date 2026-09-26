//! Managed runtime skill assets: install, preserve user edits, and remove.
use super::{claude_config_dir, codex_config_dir, write_atomic};
use std::path::{Path, PathBuf};

// ─── Runtime skill (Codex + Claude Code) ───────────────────────────────

const RUNTIME_SKILL_MD: &str = include_str!("../../../../docs/skills/pebrel-runtime/SKILL.md");
const RUNTIME_SKILL_OPENAI_YAML: &str =
    include_str!("../../../../docs/skills/pebrel-runtime/agents/openai.yaml");
const RUNTIME_SKILL_MARKER: &str = ".pebrel-managed";
const LEGACY_RUNTIME_SKILL_MARKER: &str = ".nebula-managed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManagedSkillInstall {
    Installed,
    Current,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManagedSkillRemoval {
    Removed,
    Absent,
    Conflict,
}

pub(super) fn runtime_skill_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut targets = Vec::new();
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        targets.push((
            "codex",
            PathBuf::from(profile).join(".agents").join("skills").join("pebrel-runtime"),
        ));
    }
    if let Some(claude) = claude_config_dir() {
        targets.push(("claude", claude.join("skills").join("pebrel-runtime")));
    }
    targets
}

pub(super) fn ensure_runtime_skills()
-> Vec<(&'static str, PathBuf, std::io::Result<ManagedSkillInstall>)> {
    runtime_skill_candidates()
        .into_iter()
        .filter(|(agent, _)| match *agent {
            "codex" => codex_config_dir().is_some_and(|dir| dir.exists()),
            "claude" => claude_config_dir().is_some_and(|dir| dir.exists()),
            _ => false,
        })
        .map(|(agent, path)| {
            let result = ensure_runtime_skill(&path);
            (agent, path, result)
        })
        .collect()
}

fn skill_fingerprint(skill: &[u8], metadata: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    let mut digest = Sha256::new();
    digest.update(skill);
    digest.update([0]);
    digest.update(metadata);
    digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_skill_fingerprint(dir: &Path) -> Option<String> {
    let skill = std::fs::read(dir.join("SKILL.md")).ok()?;
    let metadata = std::fs::read(dir.join("agents").join("openai.yaml")).ok()?;
    Some(skill_fingerprint(&skill, &metadata))
}

fn read_skill_marker(dir: &Path) -> Option<String> {
    [RUNTIME_SKILL_MARKER, LEGACY_RUNTIME_SKILL_MARKER]
        .into_iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok())
        .map(|value| value.trim().to_owned())
}

fn ensure_runtime_skill(dir: &Path) -> std::io::Result<ManagedSkillInstall> {
    let skill_path = dir.join("SKILL.md");
    let metadata_path = dir.join("agents").join("openai.yaml");
    let marker_path = dir.join(RUNTIME_SKILL_MARKER);
    let expected =
        skill_fingerprint(RUNTIME_SKILL_MD.as_bytes(), RUNTIME_SKILL_OPENAI_YAML.as_bytes());
    let current = read_skill_fingerprint(dir);
    let marker = read_skill_marker(dir);
    let exact_skill =
        std::fs::read(&skill_path).is_ok_and(|contents| contents == RUNTIME_SKILL_MD.as_bytes());
    let metadata_compatible = !metadata_path.exists()
        || std::fs::read(&metadata_path)
            .is_ok_and(|contents| contents == RUNTIME_SKILL_OPENAI_YAML.as_bytes());
    let empty = !skill_path.exists() && !metadata_path.exists();
    let owned = current.as_ref().zip(marker.as_ref()).is_some_and(|(a, b)| a == b);

    if !(empty || owned || (exact_skill && metadata_compatible)) {
        return Ok(ManagedSkillInstall::Conflict);
    }

    let legacy = dir.with_file_name("nebula-runtime");
    let migrating = legacy != dir && legacy.exists();
    if migrating {
        let legacy_owned = read_skill_fingerprint(&legacy)
            .zip(read_skill_marker(&legacy))
            .is_some_and(|(fingerprint, marker)| fingerprint == marker);
        if !legacy_owned {
            return Ok(ManagedSkillInstall::Conflict);
        }
        if dir.exists() {
            if remove_skill_at(&legacy)? == ManagedSkillRemoval::Conflict {
                return Ok(ManagedSkillInstall::Conflict);
            }
        } else {
            std::fs::rename(&legacy, dir)?;
        }
    }
    if !migrating
        && current.as_deref() == Some(expected.as_str())
        && std::fs::read_to_string(&marker_path).is_ok_and(|value| value.trim() == expected)
    {
        return Ok(ManagedSkillInstall::Current);
    }

    let old_marker = dir.join(LEGACY_RUNTIME_SKILL_MARKER);
    let remove_old_marker = std::fs::read_to_string(&old_marker)
        .ok()
        .zip(read_skill_fingerprint(dir))
        .is_some_and(|(marker, fingerprint)| marker.trim() == fingerprint);
    // 标记只在两份内容都原子写完后落下；崩溃不会把半套文件误认成
    // Nebula 所有，后续也绝不凭目录名覆盖用户同名 Skill。
    crate::atomic_file::write(&skill_path, RUNTIME_SKILL_MD.as_bytes())?;
    crate::atomic_file::write(&metadata_path, RUNTIME_SKILL_OPENAI_YAML.as_bytes())?;
    crate::atomic_file::write(&marker_path, format!("{expected}\n").as_bytes())?;
    if remove_old_marker {
        std::fs::remove_file(old_marker)?;
    }
    Ok(ManagedSkillInstall::Installed)
}

pub(super) fn remove_runtime_skill(dir: &Path) -> std::io::Result<ManagedSkillRemoval> {
    let current = remove_skill_at(dir)?;
    let legacy_dir = dir.with_file_name("nebula-runtime");
    if legacy_dir == dir {
        return Ok(current);
    }
    let legacy = remove_skill_at(&legacy_dir)?;
    Ok(match (current, legacy) {
        (ManagedSkillRemoval::Conflict, _) | (_, ManagedSkillRemoval::Conflict) => {
            ManagedSkillRemoval::Conflict
        },
        (ManagedSkillRemoval::Removed, _) | (_, ManagedSkillRemoval::Removed) => {
            ManagedSkillRemoval::Removed
        },
        _ => ManagedSkillRemoval::Absent,
    })
}

fn remove_skill_at(dir: &Path) -> std::io::Result<ManagedSkillRemoval> {
    let Some(marker) = read_skill_marker(dir) else {
        return Ok(ManagedSkillRemoval::Absent);
    };
    if read_skill_fingerprint(dir).as_deref() != Some(marker.as_str()) {
        return Ok(ManagedSkillRemoval::Conflict);
    }

    let marker_paths: Vec<_> = [RUNTIME_SKILL_MARKER, LEGACY_RUNTIME_SKILL_MARKER]
        .into_iter()
        .map(|name| dir.join(name))
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|value| value.trim() == marker))
        .collect();
    for path in [dir.join("SKILL.md"), dir.join("agents").join("openai.yaml")]
        .into_iter()
        .chain(marker_paths)
    {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    let metadata_dir = dir.join("agents");
    if metadata_dir.is_dir() && std::fs::read_dir(&metadata_dir)?.next().is_none() {
        std::fs::remove_dir(metadata_dir)?;
    }
    if dir.is_dir() && std::fs::read_dir(dir)?.next().is_none() {
        std::fs::remove_dir(dir)?;
    }
    Ok(ManagedSkillRemoval::Removed)
}

#[cfg(test)]
mod runtime_skill_tests {
    use super::{
        LEGACY_RUNTIME_SKILL_MARKER, ManagedSkillInstall, ManagedSkillRemoval,
        RUNTIME_SKILL_MARKER, RUNTIME_SKILL_MD, ensure_runtime_skill, remove_runtime_skill,
        skill_fingerprint,
    };

    #[test]
    fn managed_skill_installs_idempotently_and_removes_its_own_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("pebrel-runtime");

        assert_eq!(ensure_runtime_skill(&dir).unwrap(), ManagedSkillInstall::Installed);
        assert_eq!(ensure_runtime_skill(&dir).unwrap(), ManagedSkillInstall::Current);
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), RUNTIME_SKILL_MD);
        assert_eq!(remove_runtime_skill(&dir).unwrap(), ManagedSkillRemoval::Removed);
        assert!(!dir.exists());
    }

    #[test]
    fn managed_skill_never_overwrites_an_unmanaged_same_name() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("pebrel-runtime");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "user-owned\n").unwrap();

        assert_eq!(ensure_runtime_skill(&dir).unwrap(), ManagedSkillInstall::Conflict);
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), "user-owned\n");
        assert_eq!(remove_runtime_skill(&dir).unwrap(), ManagedSkillRemoval::Absent);
    }

    #[test]
    fn managed_skill_preserves_user_edits_during_update_and_remove() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("pebrel-runtime");
        assert_eq!(ensure_runtime_skill(&dir).unwrap(), ManagedSkillInstall::Installed);
        std::fs::write(dir.join("SKILL.md"), "edited after install\n").unwrap();

        assert_eq!(ensure_runtime_skill(&dir).unwrap(), ManagedSkillInstall::Conflict);
        assert_eq!(remove_runtime_skill(&dir).unwrap(), ManagedSkillRemoval::Conflict);
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            "edited after install\n"
        );
    }
    fn legacy_skill(directory: &std::path::Path) -> std::path::PathBuf {
        let legacy = directory.join("nebula-runtime");
        std::fs::create_dir_all(legacy.join("agents")).unwrap();
        std::fs::write(legacy.join("SKILL.md"), "legacy skill").unwrap();
        std::fs::write(legacy.join("agents/openai.yaml"), "legacy metadata").unwrap();
        std::fs::write(
            legacy.join(LEGACY_RUNTIME_SKILL_MARKER),
            skill_fingerprint(b"legacy skill", b"legacy metadata"),
        )
        .unwrap();
        legacy
    }

    #[test]
    fn legacy_skill_migrates_without_losing_extra_user_files() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = legacy_skill(temp.path());
        std::fs::write(legacy.join("notes.txt"), "user notes").unwrap();
        let path = temp.path().join("pebrel-runtime");
        assert_eq!(ensure_runtime_skill(&path).unwrap(), ManagedSkillInstall::Installed);
        assert!(!legacy.exists());
        assert_eq!(std::fs::read_to_string(path.join("notes.txt")).unwrap(), "user notes");
        assert!(path.join(RUNTIME_SKILL_MARKER).is_file());
        assert!(!path.join(LEGACY_RUNTIME_SKILL_MARKER).exists());
        assert_eq!(ensure_runtime_skill(&path).unwrap(), ManagedSkillInstall::Current);
    }

    #[test]
    fn edited_legacy_skill_prevents_a_second_registration() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = legacy_skill(temp.path());
        std::fs::write(legacy.join("SKILL.md"), "edited legacy skill").unwrap();
        let path = temp.path().join("pebrel-runtime");
        assert_eq!(ensure_runtime_skill(&path).unwrap(), ManagedSkillInstall::Conflict);
        assert!(!path.exists());
        assert_eq!(remove_runtime_skill(&path).unwrap(), ManagedSkillRemoval::Conflict);
        assert_eq!(
            std::fs::read_to_string(legacy.join("SKILL.md")).unwrap(),
            "edited legacy skill"
        );
    }

    #[test]
    fn new_skill_name_conflict_preserves_the_valid_legacy_skill() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = legacy_skill(temp.path());
        let path = temp.path().join("pebrel-runtime");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("SKILL.md"), "user skill").unwrap();
        assert_eq!(ensure_runtime_skill(&path).unwrap(), ManagedSkillInstall::Conflict);
        assert_eq!(std::fs::read_to_string(legacy.join("SKILL.md")).unwrap(), "legacy skill");
        assert_eq!(std::fs::read_to_string(path.join("SKILL.md")).unwrap(), "user skill");
    }
}
