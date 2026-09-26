//! The macOS participant in the shared prepare/commit/restore transaction.
use super::*;
use crate::platform::update_installation::macos as native;
use std::process::{Command, Stdio};

pub(crate) fn run_helper_if_requested() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != "--internal-macos-update") {
        return None;
    }
    if args.len() != 3 {
        return Some(2);
    }
    Some(match run(&PathBuf::from(&args[2])) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("macOS update: {error}");
            1
        },
    })
}

fn run(path: &Path) -> Result<(), String> {
    let path = canonical(path).map_err(|e| e.to_string())?;
    let directory = path.parent().ok_or("Missing update directory")?;
    let plan: Plan = read_json(&path).ok_or("Invalid update plan")?;
    let root = canonical(&nebula_settings::settings_dir().join("updates/handoffs"))
        .map_err(|e| e.to_string())?;
    if path.file_name().is_none_or(|n| n != "plan.json")
        || directory.parent() != Some(root.as_path())
        || directory.file_name().is_none_or(|n| n != plan.transaction.as_str())
        || canonical(&std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            != directory.join("handoff")
        || plan.schema != 1
        || plan.participants.len() != 1
        || plan.version != plan.asset.version
        || plan.sha256 != plan.asset.sha256.clone().unwrap_or_default()
        || canonical(&nebula_settings::settings_dir()).map_err(|e| e.to_string())?
            != plan.config_directory
        || plan.executable != plan.installation.join("Contents/MacOS/pebrel")
        || native::bundle(&plan.executable)? != plan.installation
        || plan.guard_path != guard_base(&plan.executable).with_extension("nebula-lock")
        || plan.installer != plan.config_directory.join("updates").join(&plan.asset.name)
        || !crate::update_check::can_install_version(&plan.version)?
    {
        return Err("The macOS update plan does not match this installation".into());
    }
    let _guard = crate::atomic_file::try_lifetime_lock(&guard_base(&plan.executable))
        .map_err(|e| e.to_string())?
        .ok_or("Another update owns this application")?;
    let staged = plan
        .installation
        .parent()
        .ok_or("Missing bundle parent")?
        .join(format!(".pebrel-update-{}.app", plan.transaction));
    if std::fs::symlink_metadata(&staged).is_ok() {
        return Err("The staging bundle already exists".into());
    }
    let mut swapped = false;
    let mut committed = false;
    let result: Result<(), String> = (|| {
        super::super::validate_asset(&plan.asset)?;
        let bytes = super::super::verify_file(&plan.installer, &plan.asset)?;
        if bytes != plan.bytes {
            return Err("The installer size changed".into());
        }
        let participant = &plan.participants[0];
        let created: u64 = participant.created.parse().map_err(|_| "Invalid process identity")?;
        if native::process_created(participant.pid).map_err(|e| e.to_string())? != Some(created)
            || native::running_copies(&plan.executable)? != vec![participant.pid]
        {
            return Err("Close other copies of this Pebrel application before updating".into());
        }
        native::verify_bundle(&plan.installation, &plan.original_version)?;
        let original_digest = file_digest(&plan.executable)?;
        native::stage(
            &plan.installer,
            &plan.installation,
            &staged,
            &directory.join("mount"),
            &plan.version,
        )?;
        if directory.join("cancel.json").exists() {
            return Err("Update cancelled before commit".into());
        }
        write_value(
            &directory.join("ready.json"),
            &serde_json::json!({"transaction": plan.transaction}),
        )?;
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if directory.join("cancel.json").exists() {
                return Err("Update cancelled before commit".into());
            }
            if let Some(commit) = read_json::<serde_json::Value>(&directory.join("commit.json")) {
                if commit["transaction"] != plan.transaction {
                    return Err("Update commit does not match".into());
                }
                let _: Vec<Session> = read_json(&directory.join("workspace.json"))
                    .ok_or("Missing update workspace")?;
                committed = true;
                break;
            }
            if Instant::now() >= deadline
                || native::process_created(participant.pid).map_err(|e| e.to_string())?
                    != Some(created)
            {
                return Err(
                    "The application exited or timed out without authorizing installation".into()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let deadline = Instant::now() + Duration::from_secs(90);
        while native::process_created(participant.pid).map_err(|e| e.to_string())? == Some(created)
        {
            if Instant::now() >= deadline {
                return Err("Pebrel did not exit; its application was not replaced".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !native::running_copies(&plan.executable)?.is_empty()
            || file_digest(&plan.executable)? != original_digest
        {
            return Err(
                "The installed application changed or another copy started during preparation"
                    .into(),
            );
        }
        native::verify_bundle(&staged, &plan.version)?;
        native::exchange(&plan.installation, &staged)?;
        swapped = true;
        // Persist success before launch so the new application can consume the restore ticket.
        write_result(directory, &plan, true, false, None)?;
        Ok(())
    })();
    // Startup checks this same installation lock. Release it only after the swap/result are durable.
    drop(_guard);
    let result = result.and_then(|_| relaunch(&plan, &path));
    if let Err(mut error) = result {
        let rollback_guard =
            match crate::atomic_file::try_lifetime_lock(&guard_base(&plan.executable)) {
                Ok(guard) => guard,
                Err(lock_error) => {
                    error.push_str(&format!("; could not lock for rollback: {lock_error}"));
                    None
                },
            };
        let recovery = (|| -> Result<bool, String> {
            if rollback_guard.is_none() || !native::running_copies(&plan.executable)?.is_empty() {
                return Ok(false);
            }
            if swapped {
                native::exchange(&plan.installation, &staged)?;
                Ok(true)
            } else {
                Ok(committed
                    && native::verify_bundle(&plan.installation, &plan.original_version).is_ok())
            }
        })();
        let recovered = match recovery {
            Ok(recovered) => recovered,
            Err(rollback) => {
                error.push_str(&format!(
                    "; rollback failed: {rollback}; retained bundle at {}",
                    staged.display()
                ));
                false
            },
        };
        write_result(directory, &plan, false, recovered, Some(&error))?;
        drop(rollback_guard);
        if recovered {
            let _ = relaunch(&plan, &path);
        }
        if !swapped {
            let _ = std::fs::remove_dir_all(&staged);
        }
        return Err(error);
    }
    // Retain the original signed bundle at the unique sibling path for recovery.
    Ok(())
}

fn relaunch(plan: &Plan, path: &Path) -> Result<(), String> {
    let mut child = Command::new(&plan.executable)
        .arg("--gpui")
        .env("PEBREL_CONFIG_DIR", &plan.config_directory)
        .env("NEBULA_CONFIG_DIR", &plan.config_directory)
        .env("PEBREL_UPDATE_RESTORE", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not relaunch Pebrel: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Updated Pebrel exited with {status}"))
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn write_value(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    crate::atomic_file::write(path, &serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn write_result(
    directory: &Path,
    plan: &Plan,
    success: bool,
    recovered: bool,
    error: Option<&str>,
) -> Result<(), String> {
    write_value(
        &directory.join("result.json"),
        &serde_json::json!({
            "transaction": plan.transaction, "success": success, "recovered_original": recovered, "error": error,
        }),
    )
}

fn file_digest(path: &Path) -> Result<Vec<u8>, String> {
    use sha2::{Digest as _, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests;
