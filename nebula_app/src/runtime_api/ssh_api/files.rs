use super::*;

// ---- 文件传输 ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferParams {
    asset_id: String,
    local_path: String,
    remote_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncParams {
    asset_id: String,
    local_path: String,
    remote_path: String,
    #[serde(default)]
    delete: bool,
}

pub(super) enum Direction {
    Upload,
    Download,
}

fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

fn remote_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

async fn remote_is_dir(sftp: &SftpSession, path: &str) -> bool {
    match sftp.metadata(path.to_owned()).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    }
}

/// scp 式落点：目标是已存在的目录就保留源名，否则把它当完整目标。
async fn remote_target(
    sftp: &SftpSession,
    remote_path: &str,
    source_name: &str,
) -> Result<String, ApiError> {
    if remote_path.ends_with('/') && !remote_is_dir(sftp, remote_path).await {
        return Err(ApiError::new(
            "ssh_remote_not_a_directory",
            format!("{remote_path} is not a directory"),
        )
        .details(json!({ "path": remote_path })));
    }
    if remote_is_dir(sftp, remote_path).await {
        return Ok(join_remote(remote_path, source_name));
    }
    Ok(remote_path.to_owned())
}

async fn ensure_remote_dir(
    sftp: &SftpSession,
    path: &str,
) -> Result<(), crate::ssh_session::SessionError> {
    let mut current = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            current.push('/');
            continue;
        }
        if !current.is_empty() && !current.ends_with('/') {
            current.push('/');
        }
        current.push_str(part);
        match sftp.symlink_metadata(&current).await {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                return Err(format!("not a regular remote directory: {current}").into());
            },
            Ok(_) => {},
            Err(russh_sftp::client::error::Error::Status(status))
                if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile =>
            {
                sftp.create_dir(current.clone()).await?;
            },
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn put_file(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
) -> Result<u64, crate::ssh_session::SessionError> {
    let source = tokio::fs::File::open(local).await?;
    let mut target = sftp
        .open_with_flags(
            remote.to_owned(),
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await?;
    let mut source = tokio::io::BufReader::with_capacity(TRANSFER_CHUNK, source);
    let total = tokio::io::copy_buf(&mut source, &mut target).await?;
    // 先排空在途 WRITE 应答再关句柄，否则最后一窗可能没被服务端确认。
    target.shutdown().await?;
    Ok(total)
}

async fn get_file(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
) -> Result<u64, crate::ssh_session::SessionError> {
    let source = sftp.open(remote.to_owned()).await?;
    if let Some(parent) = local.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut target = tokio::fs::File::create(local).await?;
    let mut source = tokio::io::BufReader::with_capacity(TRANSFER_CHUNK, source);
    let total = tokio::io::copy_buf(&mut source, &mut target).await?;
    target.sync_all().await?;
    Ok(total)
}

/// 递归收集本地文件（相对路径 + 绝对路径）。同步场景的目录深度是几十层量级，
/// 用阻塞遍历比在异步里再套一层 walker 简单，也不会真的卡住谁。
fn collect_local_files(root: &Path) -> std::io::Result<Vec<(String, std::path::PathBuf)>> {
    let mut files = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative =
                if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
                return Err(std::io::Error::other(format!(
                    "unsupported local entry: {}",
                    path.display()
                )));
            }
            if kind.is_dir() {
                files.push((format!("{relative}/"), path.clone()));
                stack.push((path, relative));
            } else {
                files.push((relative, path));
            }
        }
    }
    files.sort();
    Ok(files)
}

async fn upload_directory(
    sftp: &SftpSession,
    local: &Path,
    base: &str,
) -> Result<(usize, u64, std::collections::HashSet<String>), crate::ssh_session::SessionError> {
    let entries = collect_local_files(local)?;
    ensure_remote_dir(sftp, base).await?;
    let mut expected = std::collections::HashSet::new();
    let mut files = 0;
    let mut bytes = 0;
    for (relative, path) in entries {
        let remote = join_remote(base, relative.trim_end_matches('/'));
        expected.insert(remote.clone());
        if relative.ends_with('/') {
            ensure_remote_dir(sftp, &remote).await?;
        } else {
            if let Some((parent, _)) = remote.rsplit_once('/') {
                ensure_remote_dir(sftp, parent).await?;
            }
            match sftp.symlink_metadata(&remote).await {
                Ok(metadata) if metadata.is_symlink() => {
                    return Err(
                        format!("refusing to overwrite remote symbolic link: {remote}").into()
                    );
                },
                Ok(_) => {},
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile => {},
                Err(error) => return Err(error.into()),
            }
            bytes += put_file(sftp, &path, &remote).await?;
            files += 1;
        }
    }
    Ok((files, bytes, expected))
}

fn download_child(
    directory: &Path,
    name: &str,
) -> Result<std::path::PathBuf, crate::ssh_session::SessionError> {
    if name.is_empty() || name.contains(['/', '\\', ':', '\0']) || name == "." || name == ".." {
        return Err("invalid remote directory entry name".into());
    }
    Ok(directory.join(name))
}

async fn download_directory(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
) -> Result<(usize, u64), crate::ssh_session::SessionError> {
    let mut stack = vec![(remote.to_owned(), local.to_owned())];
    let mut files = 0;
    let mut bytes = 0;
    while let Some((remote, local)) = stack.pop() {
        if let Ok(metadata) = tokio::fs::symlink_metadata(&local).await {
            if metadata.file_type().is_symlink() {
                return Err(
                    format!("refusing to follow local symbolic link: {}", local.display()).into()
                );
            }
        }
        tokio::fs::create_dir_all(&local).await?;
        for entry in sftp.read_dir(&remote).await? {
            let name = entry.file_name();
            let target = download_child(&local, &name)?;
            let path = join_remote(&remote, &name);
            let metadata = sftp.symlink_metadata(&path).await?;
            if metadata.is_symlink() {
                return Err(format!("unsupported remote symbolic link: {path}").into());
            } else if metadata.is_dir() {
                stack.push((path, target));
            } else if metadata.is_regular() {
                if let Ok(metadata) = tokio::fs::symlink_metadata(&target).await {
                    if metadata.file_type().is_symlink() {
                        return Err(format!(
                            "refusing to overwrite local symbolic link: {}",
                            target.display()
                        )
                        .into());
                    }
                }
                bytes += get_file(sftp, &path, &target).await?;
                files += 1;
            } else {
                return Err(format!("unsupported remote entry: {path}").into());
            }
        }
    }
    Ok((files, bytes))
}

pub(super) fn transfer(params: &Value, direction: Direction) -> Result<Value, ApiError> {
    let params: TransferParams = parse_params(params)?;
    if params.local_path.is_empty() || params.remote_path.is_empty() {
        return Err(ApiError::invalid_params("local and remote paths must not be empty"));
    }
    let method = match direction {
        Direction::Upload => "ssh.upload",
        Direction::Download => "ssh.download",
    };
    require_ai_control(method, &params.asset_id)?;
    let local = std::path::PathBuf::from(&params.local_path);
    let is_upload = matches!(direction, Direction::Upload);
    if is_upload && !local.exists() {
        return Err(ApiError::new(
            "ssh_local_not_found",
            format!("{} does not exist", local.display()),
        ));
    }
    let started = Instant::now();
    let outcome = block_on(async {
        let sftp = crate::ssh_session::open_sftp_unattended(&params.asset_id).await?;
        let mut files = 0usize;
        let mut bytes = 0u64;
        if is_upload {
            if local.is_dir() {
                let name = local
                    .file_name()
                    .ok_or("upload source directory has no name")?
                    .to_string_lossy();
                let base = remote_target(&sftp, &params.remote_path, &name).await.map_err(
                    |error| -> crate::ssh_session::SessionError { error.message.into() },
                )?;
                (files, bytes, _) = upload_directory(&sftp, &local, &base).await?;
            } else {
                let remote = remote_target(
                    &sftp,
                    &params.remote_path,
                    &local.file_name().map_or_else(
                        || "upload".to_owned(),
                        |name| name.to_string_lossy().into_owned(),
                    ),
                )
                .await
                .map_err(|error| -> crate::ssh_session::SessionError { error.message.into() })?;
                bytes += put_file(&sftp, &local, &remote).await?;
                files = 1;
            }
        } else {
            let directory = remote_is_dir(&sftp, &params.remote_path).await;
            let target = if local.is_dir() {
                download_child(&local, remote_name(&params.remote_path))?
            } else {
                local.clone()
            };
            if directory {
                (files, bytes) = download_directory(&sftp, &params.remote_path, &target).await?;
            } else {
                bytes += get_file(&sftp, &params.remote_path, &target).await?;
                files = 1;
            }
        }
        Ok::<_, crate::ssh_session::SessionError>((files, bytes))
    });
    let mut fields = json!({
        "local_path": params.local_path,
        "remote_path": params.remote_path,
        "duration_ms": started.elapsed().as_millis() as u64,
    });
    let (files, bytes) = match outcome {
        Ok(result) => result,
        Err(error) => {
            fields["error"] = json!(error.code);
            audit(method, &params.asset_id, false, fields);
            return Err(error);
        },
    };
    fields["files"] = json!(files);
    fields["bytes"] = json!(bytes);
    audit(method, &params.asset_id, true, fields);
    Ok(json!({
        "asset_id": params.asset_id,
        "files_transferred": files,
        "bytes_transferred": bytes,
        "duration_ms": started.elapsed().as_millis() as u64,
    }))
}

/// `--delete` 的护栏：镜像删除只允许发生在明确的子目录里。
fn check_sync_target(remote_path: &str) -> Result<(), ApiError> {
    let mut parts = Vec::new();
    for part in remote_path.split('/') {
        match part {
            "" | "." => {},
            ".." => {
                parts.pop();
            },
            part => parts.push(part),
        }
    }
    let refused = parts.len() < 2;
    if refused {
        return Err(ApiError::new(
            "ssh_sync_refused",
            "refusing to mirror into / or a top-level directory; give a deeper target",
        )
        .details(json!({ "path": remote_path })));
    }
    Ok(())
}

async fn sync_base(
    sftp: &SftpSession,
    path: &str,
    delete: bool,
) -> Result<String, crate::ssh_session::SessionError> {
    let mut current = if path == "/" { "/" } else { path.trim_end_matches('/') }.to_owned();
    let mut missing = Vec::new();
    let mut base = loop {
        match sftp.canonicalize(&current).await {
            Ok(base) => break base,
            Err(russh_sftp::client::error::Error::Status(status))
                if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile =>
            {
                if current == "/" || current == "." {
                    return Err("cannot resolve remote sync root".into());
                }
                let (parent, name) = current.rsplit_once('/').unwrap_or((".", &current));
                missing.push(name.to_owned());
                current = if parent.is_empty() { "/".to_owned() } else { parent.to_owned() };
            },
            Err(error) => return Err(error.into()),
        }
    };
    for name in missing.into_iter().rev() {
        base = join_remote(&base, &name);
    }
    if delete {
        check_sync_target(&base).map_err(|error| error.message)?;
    }
    Ok(if base == "/" { base } else { base.trim_end_matches('/').to_owned() })
}

pub(super) fn sync(params: &Value) -> Result<Value, ApiError> {
    let params: SyncParams = parse_params(params)?;
    require_ai_control("ssh.sync", &params.asset_id)?;
    if params.remote_path.is_empty() {
        return Err(ApiError::invalid_params("remote path is empty"));
    }
    if params.delete {
        check_sync_target(&params.remote_path)?;
    }
    let local = std::path::PathBuf::from(&params.local_path);
    if !local.is_dir() {
        return Err(ApiError::invalid_params("sync needs a local directory"));
    }
    let started = Instant::now();
    let outcome = block_on(async {
        let sftp = crate::ssh_session::open_sftp_unattended(&params.asset_id).await?;
        let base = sync_base(&sftp, &params.remote_path, params.delete).await?;
        let (files, bytes, expected) = upload_directory(&sftp, &local, &base).await?;
        let deleted = if params.delete {
            remove_unexpected(&sftp, &base, String::new(), &expected).await?
        } else {
            0
        };
        Ok::<_, crate::ssh_session::SessionError>((files, bytes, deleted))
    });
    let mut fields = json!({
        "local_path": params.local_path,
        "remote_path": params.remote_path,
        "delete": params.delete,
        "duration_ms": started.elapsed().as_millis() as u64,
    });
    let (files, bytes, deleted) = match outcome {
        Ok(result) => result,
        Err(error) => {
            fields["error"] = json!(error.code);
            audit("ssh.sync", &params.asset_id, false, fields);
            return Err(error);
        },
    };
    fields["files"] = json!(files);
    fields["bytes"] = json!(bytes);
    fields["deleted"] = json!(deleted);
    audit("ssh.sync", &params.asset_id, true, fields);
    Ok(json!({
        "asset_id": params.asset_id,
        "files_transferred": files,
        "bytes_transferred": bytes,
        "deleted": deleted,
        "duration_ms": started.elapsed().as_millis() as u64,
    }))
}

/// 递归删除远端多余条目。返回删除的文件数。
fn remove_unexpected<'a>(
    sftp: &'a SftpSession,
    root: &'a str,
    prefix: String,
    expected: &'a std::collections::HashSet<String>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<usize, crate::ssh_session::SessionError>> + 'a>,
> {
    Box::pin(async move {
        let directory =
            if prefix.is_empty() { root.to_owned() } else { join_remote(root, &prefix) };
        let entries = sftp.read_dir(directory).await?;
        let mut removed = 0usize;
        for entry in entries {
            let name = entry.file_name();
            if name.contains(['/', '\0']) || name == "." || name == ".." {
                return Err("invalid remote directory entry name".into());
            }
            let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let path = join_remote(root, &relative);
            if sftp.symlink_metadata(&path).await?.is_dir() {
                removed += remove_unexpected(sftp, root, relative.clone(), expected).await?;
                if !expected.contains(&path) {
                    sftp.remove_dir(path).await?;
                }
            } else if !expected.contains(&path) {
                sftp.remove_file(path).await?;
                removed += 1;
            }
        }
        Ok(removed)
    })
}

#[cfg(test)]
mod tests;
