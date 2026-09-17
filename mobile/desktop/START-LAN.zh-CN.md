# Pebrel 电脑与手机局域网测试

1. 将完整便携包解压到可写目录，双击 `Connect-Phone.cmd`（连接手机）。
   工具会打开本目录的 Pebrel，首次使用时准备连接依赖，然后打开二维码页面。
2. 电脑需要 Node.js 22 或更新版本。手机与电脑连接同一个局域网。
3. 在 Android Pebrel 选择连接电脑并扫码，扫码成功后直接进入连接过程。
   不需要再填写地址、设备编号、访问密钥或证书指纹。
4. 配对页面显示当前网络和状态；多网卡时使用实际连接手机网络的地址。
   如果更换地址并生成新的二维码，手机需要重新扫码。
5. 保持连接工具窗口运行；关闭它只断开手机连接，不结束电脑其他 Tab 中的任务。
   此入口允许扫码手机输入命令，请仅向自己的手机展示二维码。

如果 Windows 询问 Node.js 防火墙权限，允许当前受信任的专用网络。
`Start-Pebrel-Preview.cmd` 仍可单独打开电脑预览版，不启动手机连接工具。

连接入口只读取本目录 `preview-profile` 的运行实例；不会自动连接已安装的其他版本。
请以普通用户运行预览版。管理员隔离实例不向此入口共享运行记录。
`mobile/relay/private` 保存电脑证书和配对密钥，请勿分享或提交。

排查时也可以在本预览版的本地终端中进入 `mobile/relay`，运行：

```powershell
npm.cmd ci --omit=dev --ignore-scripts
node pairing.mjs --lan --name My-PC --port 8765 --allow-input
```

只有需要指定网卡时才加 `--address <电脑的局域网地址>`；不要使用虚拟机专用网卡地址。
再次使用同一配对时，可加 `--config private/lan-设备编号.json`。

手机电脑会话目前支持会话列表、有界文本输出和授权命令输入，尚非完整彩色终端网格流。
二维码显示在本地浏览器配对页；原生设置页入口仍待整合。
这轮按用户要求只做打包，没有运行测试套件或模拟器，真实局域网连接由用户验收。
电脑二进制和连接工具的准确源码分别记录在随包构建说明中。

## English

Extract the full portable archive and double-click `Connect-Phone.cmd`. It opens
this adjacent Pebrel preview, prepares the pairing dependencies on first use and
opens a local QR page. Node.js 22 or newer is required. Scan from Android on the
same LAN; the phone connects without a second configuration form.

The launcher uses only this archive's `preview-profile` instance. Run it without
administrator privileges. Keep the helper window open. This entry permits command
input from the paired phone; share the QR only with your own device. Changing the
network address requires scanning the new QR code.

The QR page is currently a separate local browser tool. A native settings entry
is still pending. Desktop sessions expose bounded text and authorized input; full
terminal grid streaming is not yet available. No test suite or emulator was run
for this delivery. Build metadata distinguishes the desktop and helper sources.
