# Pebrel 自建中转测试套件

这个套件配合本次 Android 测试 APK 使用，不是公开正式 Release。需要：

- 服务器：Docker Compose、一个指向服务器的域名，开放 80/443 供 HTTPS 使用。
- 电脑：Windows / Linux / macOS、Node.js 22 或更新版本、正在运行的 Pebrel。
- 手机：本次包含“连接电脑 · 自建中转”的 APK。

手机与电脑均主动连接你的 WSS 服务器。电脑无需端口映射或 SSH 服务。
SSH 直连是 APK 内另一种独立连接方式。

## 1. 在服务器配置中转

解压测试套件，进入 `mobile/relay`。把示例域名改为你的域名，为第一台电脑生成配置：

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" -w /work \
  node:22-alpine node init.mjs \
  --url wss://relay.your-domain.com --name Windows-PC --output private
```

命令生成三个文件，不把连接密钥打印到终端：

- `private/relay.config.json`：仅保留在服务器，中转服务读取它。
- `private/computer-设备编号.json`：通过你信任的方式传到对应电脑。
- `private/phone-设备编号.txt`：在手机添加连接时粘贴其内容。

配置使用不同的随机手机／电脑密钥，不要将它们加入 Git 或分享给其他人。
启动中转，并把检查命令中的域名改为你的域名；Caddy 自动申请和续期 HTTPS 证书：

```sh
docker compose up -d --build
curl --fail https://relay.your-domain.com/healthz
```

最后一条应返回 `ok`。初始化命令自动生成 `.env`，保存域名与生成文件的用户 ID；
Compose 据此读取权限受限的配置文件。服务器只需 Docker，无需另外安装 Node。
这套 Compose 使用标准 443 端口；域名解析和证书申请需要服务器能够访问外网。

## 2. 在电脑启动连接工具

把同一套件解压到电脑，进入 `mobile/relay`，并把该电脑的 JSON 配置放到这里。
在 Pebrel 的一个**本地终端 Tab** 中执行：

```sh
npm ci --omit=dev --ignore-scripts
node connector.mjs --config ./computer-设备编号.json --allow-input
```

Windows PowerShell、Linux 和 macOS 使用相同的 Node 命令。电脑名称由生成配置时
的 `--name` 决定。`--allow-input` 表示允许已持有该手机凭据的连接发送命令；
省略它就是只读。连接工具使用 Pebrel 现有的本机 Runtime API，密钥不会传给手机。
它可以与支持这些 Runtime 操作的既有 Pebrel 构建一起工作，不要求新增
`mobile-bridge` 命令；该命令只用于 APK 的“通过 SSH 连接 Pebrel”路径。

状态应依次显示 `connecting`、`waiting_for_phone`、`paired`。保持此连接工具运行。
退出连接工具只断开移动连接，不结束其他 Pebrel Tab 中的任务。

默认使用本地 Tab 继承的 `PEBREL_RUNTIME_ENDPOINT`，否则读取标准数据目录的
`runtime.port`。特殊便携目录可以设置 `PEBREL_CONFIG_DIR`，或在电脑 JSON 中加入
`"runtimeFile": "实际的 runtime.port 完整路径"`。不匹配时连接失败，不会另找一台实例。

## 3. 显示配对二维码

电脑端可以用 Node 启动一个 Pebrel 风格的本地浏览器配对页。它读取已有的电脑配置
和手机邀请，二维码由本地固定版本的 `qrcode` 库生成；页面只绑定 `127.0.0.1`，
不会把邀请或其中的密钥暴露到局域网或公共网页：

```sh
npm ci --omit=dev --ignore-scripts
node pairing.mjs \
  --config ./computer-设备编号.json \
  --invitation ./phone-设备编号.txt \
  --allow-input
```

命令会打开 `http://127.0.0.1:随机端口/...`，在页面中显示实际二维码，并在终端和
页面中显示 `connecting`、`waiting_for_phone`、`paired` 等状态。`--allow-input` 仍然
是明确的电脑端权限开关；省略它时手机只能读取。没有桌面环境时可加 `--no-browser`，
再把终端打印的回环地址复制到本机浏览器。二维码页的复制按钮只复制当前邀请，且
页面关闭后回环服务随之停止。

## 4. 局域网直连模式

没有自建服务器时，可以让电脑启动同一套有界中转协议的本地 TLS 端点。电脑绑定
选定的局域网地址和端口，手机通过 `wss://局域网地址:端口` 连接；连接工具仍从
电脑主动接入本机端点，因此 Pebrel Runtime 令牌不会进入邀请或手机。首次运行：

```sh
npm ci --omit=dev --ignore-scripts
node pairing.mjs --lan --name Office-PC --address 192.168.1.42 --port 8765
```

不传 `--address` 时选择第一个非回环 IPv4 地址；可用 `--advertise` 指定手机实际
访问的 IP 或局域网主机名，`--port 0`（默认）让系统分配可用端口。需要输入权限时再加
`--allow-input`。命令会在 `private/lan-设备编号.json` 保存电脑端配置，在
`private/phone-设备编号.txt` 保存手机邀请，并打开包含二维码的本地页面：

- `lan-设备编号.json` 包含自签名证书和私钥，权限为 600，只留在这台电脑。
- 手机邀请沿用 v1 JSON：`version`、`url`、`device`、43 字符 `token`、`name`，并
  添加 `mode: "lan"` 与 `tlsPin: "sha256/<Base64 SPKI>"`。
- 手机和电脑都校验证书有效期、所选地址的 SAN 以及完全匹配的 SPKI 指纹。任何
  指纹变化都中止握手；代码不会关闭全局 TLS 校验，也不会使用全局信任所有证书。
- `--address` 是监听地址；绑定 `0.0.0.0` 或 `::` 时必须用 `--advertise`（或让
  工具选择一个地址）生成二维码，电脑连接工具通过临时的本机回环入口接入该
  HTTPS 监听器。

局域网端点没有公共 HTTPS 证书，也不应转发到互联网。手机扫描二维码后，在同一
个受信任网络中完成一次配对；要更换证书或设备凭据，删除对应的 `lan-*.json` 后
重新运行。Node 22 或更新版本、Windows/Linux/macOS 均使用同一条命令。

## 5. 在手机连接

1. 打开 APK，在“电脑”区选择“连接电脑 · 自建中转”。
2. 粘贴对应 `phone-设备编号.txt` 的全部 JSON 内容，点击连接。
3. 进入电脑列表后点一个 Tab，查看当前任务和输出。
4. 若电脑连接工具使用了 `--allow-input`，可在本地编辑命令再发送；可以先测 `pwd`
   或 `Get-Location`。选择真实 Shell Tab，避免把测试命令发进不对应的交互程序。

电脑页有断开按钮；断开后首页保留连接配置，点它可重新连接。凭据保存在 Android
Keystore 保护的加密数据中。当前一次允许一部手机连接一台电脑；重复连接会被拒绝。
不同电脑可以同时连接。

## 多台电脑和撤销

在服务器再次执行初始化命令并换一个名称，会向同一服务器配置增加一台设备：

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" -w /work \
  node:22-alpine node init.mjs \
  --url wss://relay.your-domain.com --name MacBook --output private
docker compose restart relay
```

把新生成的电脑配置交给 MacBook，手机导入对应的新文件。不要把同一个电脑配置
同时用在两台电脑上。

撤销时从服务器 `private/relay.config.json` 删除对应设备后执行
`docker compose restart relay`。已有连接随重启断开，旧密钥无法重新连接。
重新添加会生成新的设备 ID 和两把新密钥。

## 本次可以验证的内容与边界

- 身份认证、手机/电脑角色隔离、多个设备、原有 Tab 枚举、输出读取、输入和实时任务状态。
- 当前打开的 Tab 在前台约每 2 秒读取一次有界文本；SSH 终端使用实时字节流。
  电脑路径还没有彩色网格流或独占接管。
- 手机断开不会停止电脑任务。电脑连接工具支持带随机抖动的退避重连；手机可点保存的设备重连。
- 所有待确认输入在断线后标记为结果未知，不自动重发。新连接使用新的连接 ID，旧帧不能进入新会话。
- 同一连接的输入 ID 不会因输出轮询被淘汰；单次配对最多接受 4,096 次输入，达到后需主动重连。
- WebSocket 单消息和发送缓存有硬上限；禁用压缩，空闲只保活，不轮询全部 Tab。
- TLS 校验你自己的服务器证书。**本次服务器是受信任的中转终点，管理员能够读取会话内容；尚无端到端加密。**
- 通知来自在线状态订阅；尚无离线通知持久补收，也没有接入官方推送服务器。
- Node 连接工具和浏览器配对页目前单独启动，尚未集成成桌面“移动设备”设置页。
  后台服务开关只能改善 Android 存活，不保证强行停止、系统终止或关机后仍能收通知。

## Verification / 验证

`npm test` 使用真实 WebSocket 和 loopback TCP，验证角色密钥、重复登录、Runtime
请求与订阅、输入权限、重复请求和旧连接重放拒绝。CI 同时构建 Docker 镜像并校验
Compose 配置。Android 对 WSS 邀请、TLS/RPC 生命周期另有测试。实际运营网络的
HTTPS/DNS 连通性以及手机耗电、流畅度仍需你在设备上验收。

Sources use GPLv3-compatible terms. This relay uses `ws` (MIT). The browser pairing
helper pins `qrcode` 1.5.4 and `selfsigned` 2.4.1; their licenses, the transitive
`node-forge` license, and integrity values are recorded in
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) and `package-lock.json`. The
browser page has no external CDN dependency, and the relay contains no copied
proprietary server source or deployable cloud service.
