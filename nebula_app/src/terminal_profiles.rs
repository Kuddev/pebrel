//! User terminal profiles discovered from a directory or saved explicitly.

use std::io;
#[cfg(windows)]
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STORE_VERSION: u32 = 1;
const STORE_FILE: &str = "terminal_profiles.json";

/// A launchable terminal profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalProfile {
    /// Stable identity: discovered commands use a path id, custom sessions a random id.
    pub id: String,
    pub name: String,
    /// Absolute executable path.
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Optional integration override; absent infers recognized executable names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::profile_environment::ProfileEnvironment::is_empty"
    )]
    pub env: crate::profile_environment::ProfileEnvironment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileStore {
    version: u32,
    #[serde(default)]
    profiles: Vec<TerminalProfile>,
}

/// Versioned collection stored in `terminal_profiles.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TerminalProfiles {
    profiles: Vec<TerminalProfile>,
}

impl TerminalProfiles {
    pub(crate) fn load() -> io::Result<Self> {
        Self::load_from(&store_path())
    }

    pub(crate) fn load_from(path: &Path) -> io::Result<Self> {
        let mut recorded = Self::read_recorded_from(path)?;
        recorded.profiles.retain(|profile| command_is_launchable(&profile.command));
        Ok(recorded)
    }

    fn read_recorded_from(path: &Path) -> io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        };
        let store: ProfileStore = serde_json::from_slice(&bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if store.version != STORE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported terminal profile version {}", store.version),
            ));
        }
        for profile in &store.profiles {
            validate(profile)?;
        }
        // Keep unavailable commands on disk. Presentation may hide them, but an
        // unrelated edit must not erase profiles for a disconnected installation.
        Ok(Self { profiles: store.profiles })
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        self.save_to(&store_path())
    }

    pub(crate) fn save_to(&self, path: &Path) -> io::Result<()> {
        for profile in &self.profiles {
            validate(profile)?;
        }
        let Some(_lock) = crate::atomic_file::try_lock(path)? else {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "terminal profile store is locked by another process",
            ));
        };
        self.write_locked(path)
    }

    fn write_locked(&self, path: &Path) -> io::Result<()> {
        let store = ProfileStore { version: STORE_VERSION, profiles: self.profiles.clone() };
        let bytes = serde_json::to_vec_pretty(&store).map_err(io::Error::other)?;
        crate::atomic_file::write(path, &bytes)
    }

    /// Compare the selected row under the store lock; unrelated edits survive.
    pub(crate) fn edit_at(
        path: &Path,
        expected: Option<&TerminalProfile>,
        replacement: Option<TerminalProfile>,
    ) -> io::Result<Self> {
        let Some(_lock) = crate::atomic_file::try_lock(path)? else {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "terminal profile store is busy",
            ));
        };
        let mut store = Self::read_recorded_from(path)?;
        if let Some(expected) = expected {
            if store.profiles.iter().find(|profile| profile.id == expected.id) != Some(expected) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "terminal profile changed; reload before editing",
                ));
            }
            if replacement.as_ref().is_some_and(|profile| profile.id != expected.id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "profile identity changed during edit",
                ));
            }
            store.profiles.retain(|profile| profile.id != expected.id);
        } else if replacement
            .as_ref()
            .is_some_and(|profile| store.profiles.iter().any(|existing| existing.id == profile.id))
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "profile identity already exists",
            ));
        }
        if let Some(profile) = replacement {
            store.upsert(profile)?;
        }
        store.write_locked(path)?;
        store.profiles.retain(|profile| command_is_launchable(&profile.command));
        Ok(store)
    }

    pub(crate) fn new_id() -> io::Result<String> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(format!("custom-{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
    }

    /// Copy a detected preset's raw options, before platform integration is injected.
    /// The editor saves this as a new identity through the existing CAS transaction.
    pub(crate) fn from_detected(
        shell: &crate::shell_detect::DetectedShell,
    ) -> io::Result<TerminalProfile> {
        Ok(TerminalProfile {
            id: Self::new_id()?,
            name: shell.name.clone(),
            command: shell.program.clone().into(),
            args: shell.args.clone(),
            cwd: None,
            shell_id: Some(shell.id.clone()),
            env: Default::default(),
        })
    }

    pub(crate) fn profiles(&self) -> &[TerminalProfile] {
        &self.profiles
    }

    pub(crate) fn as_config_profiles(&self) -> Vec<crate::config::ui_config::Profile> {
        self.profiles
            .iter()
            .map(|profile| crate::config::ui_config::Profile {
                name: profile.name.clone(),
                command: profile.command.to_string_lossy().into_owned(),
                args: profile.args.clone(),
                cwd: profile.cwd.clone(),
                shell_id: profile.shell_id.clone().or_else(|| infer_shell_id(&profile.command)),
                terminal_profile_id: Some(profile.id.clone()),
                env: profile.env.clone(),
            })
            .collect()
    }

    /// Replace a stable profile identity; different sessions may share an executable.
    pub(crate) fn upsert(&mut self, profile: TerminalProfile) -> io::Result<()> {
        validate(&profile)?;
        if let Some(existing) = self.profiles.iter_mut().find(|existing| existing.id == profile.id)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        Ok(())
    }
}

#[cfg(test)]
mod preset_tests;

/// Scan only `directory` and its immediate `bin` child for known Windows shells.
pub(crate) fn scan_directory(directory: &Path) -> io::Result<Vec<TerminalProfile>> {
    let directory = directory.canonicalize()?;
    if !directory.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotADirectory, "scan path is not a directory"));
    }

    let roots = [directory.clone(), directory.join("bin")];
    let shells = [
        ("pwsh.exe", "PowerShell 7", "pwsh", &["-NoLogo"][..]),
        ("powershell.exe", "Windows PowerShell", "powershell", &["-NoLogo"][..]),
        ("bash.exe", "Git Bash", "bash", &["--login", "-i"][..]),
        ("nu.exe", "Nushell", "nu", &[][..]),
        ("cmd.exe", "Command Prompt", "cmd", &[][..]),
    ];
    let mut profiles = Vec::new();

    for (file_name, name, shell_id, args) in shells {
        for root in &roots {
            let candidate = root.join(file_name);
            if !candidate.is_file() {
                continue;
            }
            let command = candidate.canonicalize()?;
            if !command_is_launchable(&command) {
                continue;
            }
            if profiles
                .iter()
                .any(|profile: &TerminalProfile| commands_equal(&profile.command, &command))
            {
                continue;
            }
            profiles.push(TerminalProfile {
                id: stable_id(shell_id, &command),
                name: name.to_owned(),
                command,
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                cwd: None,
                shell_id: Some(shell_id.to_owned()),
                env: Default::default(),
            });
        }
    }
    Ok(profiles)
}

/// Shell family is optional for custom executables. Only recognized basenames
/// select an integration adapter; command names never need a user-supplied id.
pub(crate) fn infer_shell_id(command: &Path) -> Option<String> {
    let text = command.to_string_lossy();
    let name = text.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(name, "zsh" | "bash" | "fish" | "pwsh" | "powershell" | "cmd" | "nu" | "sh")
        .then(|| name.to_owned())
}

pub(crate) fn store_path() -> PathBuf {
    crate::platform::dirs::data_dir().join(STORE_FILE)
}

fn stable_id(shell_id: &str, command: &Path) -> String {
    let command = command.to_string_lossy();
    #[cfg(windows)]
    let command = command.to_ascii_lowercase();
    let digest = Sha256::digest(command.as_bytes());
    let suffix = digest[..8].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("{shell_id}-{suffix}")
}

fn commands_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn command_is_launchable(command: &Path) -> bool {
    if !command.is_file() {
        return false;
    }

    #[cfg(windows)]
    {
        pe_machine(command).is_ok_and(machine_is_supported)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
fn pe_machine(command: &Path) -> io::Result<u16> {
    let mut file = std::fs::File::open(command)?;
    let mut dos = [0_u8; 64];
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing DOS executable header"));
    }

    // PE 的 Machine 字段不在固定文件偏移：先从 DOS 头 0x3c 读取 e_lfanew，
    // 再核对 PE 签名，避免把安装目录名里的 x64/arm64 当成可信架构信息。
    let pe_offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    file.seek(SeekFrom::Start(pe_offset))?;
    let mut header = [0_u8; 6];
    file.read_exact(&mut header)?;
    if &header[..4] != b"PE\0\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing PE signature"));
    }
    Ok(u16::from_le_bytes([header[4], header[5]]))
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn machine_is_supported(machine: u16) -> bool {
    matches!(machine, 0x014c | 0x8664)
}

#[cfg(all(windows, target_arch = "x86"))]
fn machine_is_supported(machine: u16) -> bool {
    machine == 0x014c
}

#[cfg(all(windows, target_arch = "aarch64"))]
fn machine_is_supported(machine: u16) -> bool {
    matches!(machine, 0x014c | 0x8664 | 0xaa64)
}

#[cfg(all(
    windows,
    not(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64"))
))]
fn machine_is_supported(_machine: u16) -> bool {
    true
}

fn validate(profile: &TerminalProfile) -> io::Result<()> {
    profile.env.validate().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if profile.id.trim().is_empty()
        || profile.name.trim().is_empty()
        || profile.shell_id.as_ref().is_some_and(|id| id.trim().is_empty())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal profile id, name, and explicit shell_id must not be empty",
        ));
    }
    if !profile.command.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal profile command must be absolute",
        ));
    }
    if profile.args.iter().any(|arg| arg.contains('\0')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal profile argument contains NUL",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TerminalProfile, TerminalProfiles, scan_directory};

    #[cfg(windows)]
    fn write_pe(path: &std::path::Path, machine: u16) {
        let mut bytes = vec![0_u8; 0x86];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&(0x80_u32).to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    const TEST_MACHINE: u16 = 0x8664;
    #[cfg(all(windows, target_arch = "x86"))]
    const TEST_MACHINE: u16 = 0x014c;
    #[cfg(all(windows, target_arch = "aarch64"))]
    const TEST_MACHINE: u16 = 0xaa64;

    #[test]
    fn scans_selected_directory_and_bin_in_stable_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("bin")).unwrap();
        #[cfg(windows)]
        {
            write_pe(&temp.path().join("cmd.exe"), TEST_MACHINE);
            write_pe(&temp.path().join("bin").join("pwsh.exe"), TEST_MACHINE);
        }
        #[cfg(not(windows))]
        {
            std::fs::write(temp.path().join("cmd.exe"), b"").unwrap();
            std::fs::write(temp.path().join("bin").join("pwsh.exe"), b"").unwrap();
        }
        std::fs::write(temp.path().join("ignored.exe"), b"").unwrap();

        let profiles = scan_directory(temp.path()).unwrap();
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.shell_id.as_deref().unwrap_or("custom"))
                .collect::<Vec<_>>(),
            ["pwsh", "cmd"]
        );
        assert!(profiles.iter().all(|profile| profile.command.is_absolute()));
        assert_eq!(profiles[0].name, "PowerShell 7");
        assert_eq!(profiles[0].args, ["-NoLogo"]);
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn scan_skips_arm64_shells_in_an_x64_build() {
        let temp = tempfile::tempdir().unwrap();
        write_pe(&temp.path().join("pwsh.exe"), 0xaa64);

        assert!(scan_directory(temp.path()).unwrap().is_empty());
    }

    #[test]
    fn json_store_roundtrips_and_reports_malformed_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles.json");
        let profile = profile(temp.path().join("pwsh.exe"), "first");
        #[cfg(windows)]
        write_pe(&profile.command, TEST_MACHINE);
        #[cfg(not(windows))]
        std::fs::write(&profile.command, b"").unwrap();
        let mut profiles = TerminalProfiles::default();
        profiles.upsert(profile.clone()).unwrap();
        profiles.save_to(&path).unwrap();

        assert_eq!(TerminalProfiles::load_from(&path).unwrap().profiles(), [profile]);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(
            TerminalProfiles::load_from(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn upsert_deduplicates_commands() {
        let temp = tempfile::tempdir().unwrap();
        let command = temp.path().join("pwsh.exe");
        let mut profiles = TerminalProfiles::default();
        profiles.upsert(profile(command.clone(), "first")).unwrap();

        #[cfg(windows)]
        let duplicate = std::path::PathBuf::from(command.to_string_lossy().to_ascii_uppercase());
        #[cfg(not(windows))]
        let duplicate = command;
        profiles.upsert(profile(duplicate, "replacement")).unwrap();

        assert_eq!(profiles.profiles().len(), 1);
        assert_eq!(profiles.profiles()[0].name, "replacement");
    }

    #[test]
    fn local_profile_environment_store_preserves_same_executable_sessions_and_detects_stale_edits()
    {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles.json");
        let command = std::env::current_exe().unwrap();
        let mut alpha = profile(command.clone(), "Alpha");
        alpha.id = TerminalProfiles::new_id().unwrap();
        alpha.env = crate::profile_environment::ProfileEnvironment::from_pairs([
            ("QA_VALUE".into(), "alpha".into()),
            ("EMPTY".into(), String::new()),
        ])
        .unwrap();
        let mut beta = alpha.clone();
        beta.id = TerminalProfiles::new_id().unwrap();
        beta.name = "Beta".into();
        beta.env.0.insert("QA_VALUE".into(), "beta".into());
        TerminalProfiles::edit_at(&path, None, Some(alpha.clone())).unwrap();
        let store = TerminalProfiles::edit_at(&path, None, Some(beta.clone())).unwrap();
        assert_eq!(store.profiles().len(), 2);
        assert_eq!(TerminalProfiles::load_from(&path).unwrap(), store);
        let config = store.as_config_profiles();
        assert_ne!(config[0].settings_id(), config[1].settings_id());
        assert_eq!(config[0].env.0["EMPTY"], "");
        let mut edited = alpha.clone();
        edited.env.0.insert("QA_VALUE".into(), "next-launch".into());
        let edited_store =
            TerminalProfiles::edit_at(&path, Some(&alpha), Some(edited.clone())).unwrap();
        let stale = TerminalProfiles::edit_at(&path, Some(&alpha), None).unwrap_err();
        assert_eq!(stale.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(TerminalProfiles::load_from(&path).unwrap(), edited_store);
        let final_store = TerminalProfiles::edit_at(&path, Some(&edited), None).unwrap();
        assert_eq!(final_store.profiles(), &[beta]);
        // Older v1 records without an env map still load with an empty overlay.
        let mut old = serde_json::to_value(&super::ProfileStore {
            version: 1,
            profiles: final_store.profiles().to_vec(),
        })
        .unwrap();
        old["profiles"][0].as_object_mut().unwrap().remove("env");
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(TerminalProfiles::load_from(&path).unwrap().profiles()[0].env.is_empty());
    }

    #[test]
    fn local_profile_environment_edit_preserves_hidden_unavailable_commands() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles.json");
        let hidden = profile(temp.path().join("disconnected-shell"), "Unavailable");
        let mut recorded = TerminalProfiles::default();
        recorded.upsert(hidden.clone()).unwrap();
        recorded.save_to(&path).unwrap();
        assert!(TerminalProfiles::load_from(&path).unwrap().profiles().is_empty());
        let mut available = profile(std::env::current_exe().unwrap(), "Available");
        available.id = "distinct-available".into();
        let visible = TerminalProfiles::edit_at(&path, None, Some(available.clone())).unwrap();
        assert_eq!(visible.profiles(), &[available.clone()]);
        let recorded: super::ProfileStore =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(recorded.profiles, [hidden, available]);
    }

    #[test]
    fn local_profile_environment_infers_known_shells_and_allows_unknown_executables() {
        for (path, expected) in [
            ("/bin/zsh", "zsh"),
            ("/bin/bash", "bash"),
            ("/usr/bin/fish", "fish"),
            (r"C:\Tools\PWSH.EXE", "pwsh"),
            (r"C:\Windows\CMD.exe", "cmd"),
        ] {
            assert_eq!(
                super::infer_shell_id(std::path::Path::new(path)).as_deref(),
                Some(expected)
            );
        }
        assert!(super::infer_shell_id(std::path::Path::new("/opt/custom/worker")).is_none());
        let mut custom = profile(std::env::current_exe().unwrap(), "Custom");
        custom.shell_id = None;
        let mut profiles = TerminalProfiles::default();
        profiles.upsert(custom).unwrap();
        let profile = &profiles.as_config_profiles()[0];
        assert!(profile.shell_id.is_none());
        assert_eq!(profile.settings_id().as_deref(), Some("profile:custom|pwsh-test"));
        assert!(profile.matches_settings_id("profile:zsh|pwsh-test"));
        assert!(!profile.matches_settings_id("profile:custom|PWSH-TEST"));
        let stored = TerminalProfile {
            id: "custom-known".into(),
            name: "Known".into(),
            command: std::env::current_exe().unwrap().parent().unwrap().join("zsh"),
            args: vec![],
            cwd: None,
            shell_id: None,
            env: Default::default(),
        };
        let mut inferred = TerminalProfiles::default();
        inferred.upsert(stored).unwrap();
        assert_eq!(inferred.as_config_profiles()[0].shell_id.as_deref(), Some("zsh"));
        assert!(inferred.as_config_profiles()[0].matches_settings_id("profile:bash|custom-known"));
    }

    fn profile(command: std::path::PathBuf, name: &str) -> TerminalProfile {
        TerminalProfile {
            id: "pwsh-test".into(),
            name: name.into(),
            command,
            args: vec!["-NoLogo".into()],
            cwd: None,
            shell_id: Some("pwsh".into()),
            env: Default::default(),
        }
    }
}
