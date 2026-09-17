# Pebrel 电脑与手机局域网测试

这是 mobile 分支的 Windows x64 便携测试包，包含同一提交的电脑程序和二维码连接工具。
源码提交在 `SOURCE_COMMIT` 中；测试模式和文件校验值在 `BUILD.json` 中。

1. 将整个 ZIP 解压到可写目录，双击 `Start-Pebrel-Preview.cmd`。
   它在包旁的 `preview-profile` 使用独立配置，避免连接到已安装版本的旧实例。
2. 电脑需要 Node.js 22 或更新版本。手机与电脑连接同一个局域网。
3. 在**刚启动的 Pebrel 的本地 PowerShell 终端**中进入本包的 `mobile/relay` 目录：

   ```powershell
   cd '本包解压目录\mobile\relay'
   npm.cmd ci --omit=dev --ignore-scripts
   node pairing.mjs --lan --name My-PC --port 8765 --allow-input
   ```

4. 浏览器会显示这台电脑的二维码。在 Android Pebrel 的电脑连接入口选择局域网并扫描，
   点击连接。若 Windows 询问 Node.js 防火墙权限，允许当前受信任的专用网络。
5. 在手机上打开电脑的一个普通 Shell 会话，先查看输出，再发送 `Get-Location`。
   配对命令所在 Tab 应保持运行，选择另一个普通 Shell Tab 进行输入测试。

多网卡或 VPN 下，在上面的命令加上 `--address 192.168.1.42`，使用电脑实际的局域网 IP。
`--allow-input` 允许持有邀请的手机发送命令；去掉它则为只读模式。
关闭配对命令会断开手机连接，不会结束电脑其他 Tab 中的任务。

再次使用同一配对时，在命令中加上 `--config private/lan-设备编号.json`。
`private` 保存这台电脑的证书和配对密钥，请勿分享或提交。

这轮按要求仅构建打包，没有运行测试套件或模拟器验收，真实局域网连接由你验收。
电脑二维码目前显示在随包工具打开的本地浏览器页，尚未加入原生设置页。
手机电脑会话目前支持会话列表、有界文本输出和授权命令输入，尚非完整彩色终端网格流。
更多中转配置见 `mobile/relay/README.md`。

## English

Extract the full portable archive and launch `Start-Pebrel-Preview.cmd`. It uses a
separate `preview-profile` beside the executable. Install Node.js 22 or later,
then run the commands above in this preview's local terminal. Scan the QR from
Android on the same LAN. Use another ordinary shell tab for command input.

This manual preview is built without test suites. The QR page is a separate local
browser tool. Desktop sessions currently expose bounded text and authorized
command input, rather than a full color terminal grid stream.
