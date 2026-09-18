# Pebrel Relay — 0.4.10 preview

## English

### Added

- A downloadable shell installer obtains the correct Linux x64/ARM64 executable,
  verifies its pinned SHA256, starts the service, and checks local TLS readiness.
  It provides status, start, stop and ownership-checked uninstall commands.
- The script defaults to TCP 443 and selects 8443 when 443 is occupied. It stops
  if the selected port is also occupied; it does not stop other services.

### Fixed

- Installation supports systemd 239–246 without unsupported credential directives.
  The listener drops root privileges before accepting connections. Newer systemd
  retains DynamicUser and credential passing; OpenRC remains supported.
- A normal RHEL-family `/etc/init.d` directory symlink no longer blocks a systemd
  install. Actual target symlinks and foreign service files remain protected.
- The companion Android preview reports upload completion only after remote
  integrity verification, rather than when the sender finishes writing bytes.

## 中文

### 新增

- 可下载的 Shell 安装脚本自动获取 Linux x64/ARM64 程序，核对固定 SHA256，
  启动服务并检查本机加密连接；提供状态、启停和带所有权校验的卸载命令。
- 脚本默认使用 TCP 443，占用时改用 8443；所选端口也被占用时停止，
  不会结束其他服务。

### 修复

- 支持 systemd 239–246，不使用其尚未提供的凭据指令。监听程序在接受连接前
  永久降权；新版 systemd 保留动态用户与凭据传递，继续支持 OpenRC。
- RHEL 系统正常的 `/etc/init.d` 目录链接不再阻止 systemd 安装；
  实际安装目标的符号链接和其他服务文件仍受保护。
- 配套 Android 预览版等待服务器校验通过后才报告上传完成，不再只依据发送进度。

This is a testing prerelease, separate from desktop releases. No Docker/Node.js
is required on the server. Firewall/cloud rules are not changed. Local readiness
does not prove external reachability. Existing different versions are not
automatically upgraded; uninstall retains credentials unless purge is explicit.

这是独立于桌面正式版本的测试预发布。服务器无需 Docker/Node.js，不改防火墙和
云安全组。本机就绪不代表公网可达；已有不同版本不自动升级，卸载默认保留凭据，
只有明确清理时才删除。

## SHA256

Final asset checksums are supplied in `SHA256SUMS` after the CI build.
最终资产校验值在 CI 构建后的 `SHA256SUMS` 中提供。
