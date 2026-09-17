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

## 3. 在手机连接

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
- Node 连接工具目前单独启动，尚未集成成桌面“移动设备”设置页。后台服务开关
  只能改善 Android 存活，不保证强行停止、系统终止或关机后仍能收通知。

## Verification / 验证

`npm test` 使用真实 WebSocket 和 loopback TCP，验证角色密钥、重复登录、Runtime
请求与订阅、输入权限、重复请求和旧连接重放拒绝。CI 同时构建 Docker 镜像并校验
Compose 配置。Android 对 WSS 邀请、TLS/RPC 生命周期另有测试。实际运营网络的
HTTPS/DNS 连通性以及手机耗电、流畅度仍需你在设备上验收。

Sources use GPLv3-compatible terms. This relay uses `ws` (MIT); its pinned version
and integrity are recorded in package-lock.json, and its license is retained by
npm. The relay contains no copied proprietary server source and does not provide
a deployable cloud service.
