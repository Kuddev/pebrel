# Pebrel 0.4.9 设置页测试 / Settings preview

## 中文

解压到新的可写目录，运行 `Start-Pebrel-Preview.cmd`。此入口使用独立的
`preview-profile`，不替换已安装版本。保留旧测试目录，不要删除原有配置。

- 手机连接：设置内显示局域网 / 中转选项和原生二维码；首次进入局域网页
  自动准备二维码。宽窗口左右布局，窄窗口优先显示二维码。手动参数收进高级。
- SSH：主机卡片使用线框图标容器，保留系统图标；搜索、筛选和分组集中在列表上方。
- 云备份：设置 → 备份，提供备份记录与云存储两个页签。同步内容在配置卡片内展开，
  普通配置修改自动保存；凭据需点击保存，备份密码仅本次使用。
- 备份列表来自真实存储查询，可恢复指定备份；恢复前会确认覆盖。现有能力是整包
  加密备份，保留最近 10 份，不包含 SSH 私钥，不是自动上传或增量同步。
- 配套 Android 0.4.9 将首页设置按钮换为常规齿轮。本轮不要求重新安装中转服务，
  继续使用已有服务器地址与自定义端口。

重点检查：二维码是否醒目、窄窗口是否可用、SSH 筛选按钮留白是否可点击、
云存储输入后是否显示“已保存”、关闭设置重开后同步范围是否保留。
先使用测试备份目录；不要用唯一一份重要数据测试恢复覆盖。

这是分支测试包，不是正式发布。编译、定向检查与实际设备视觉验收是不同证据；
请以对应 CI 结果为准。真实网络、不同 DPI、浅深色主题仍需此次人工测试。
不要发送二维码、密钥、密码或完整连接配置。源码与哈希见 `SOURCE_COMMIT` / `BUILD.json`。

## English

Extract into a new writable directory and launch `Start-Pebrel-Preview.cmd`.
It uses a separate `preview-profile`; retain earlier previews and their data.

- Mobile settings put native QR pairing beside LAN / relay setup on wide windows,
  or above it on narrow windows. LAN prepares automatically; manual fields live in Advanced.
- SSH host cards preserve OS glyphs inside bordered anchors, with compact filtering and groups.
- Backup settings provide history and cloud-storage tabs. Backup contents expand inside
  the configuration card; ordinary edits autosave. Credentials require Save; the backup
  password is session-only. Real snapshots can be restored after overwrite confirmation.
- Backups remain encrypted full archives with 10 retained copies and no SSH private keys;
  this is not incremental sync or automatic upload.
- Android 0.4.9 uses a conventional settings gear. Existing relay services and custom ports
  remain valid; this UI preview does not require a server reinstall.

Check QR prominence, narrow layouts, full filter hit targets, save feedback and persisted scope.
Use disposable backup data when testing restore. Refer to CI for targeted check results;
real-device networking, theme/DPI and visual acceptance still require manual review.
