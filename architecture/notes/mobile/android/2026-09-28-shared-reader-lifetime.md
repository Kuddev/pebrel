# 文件与对话共用阅读器的资源边界

## Status

Implemented.

## Context

电脑文件、SFTP 文件和 Agent 消息都需要 Markdown、代码复制及链接跳转。
分别维护三个渲染器会让中文、代码块、主题和复制反馈逐渐不一致；直接让远端正文
控制 WebView 的导航或资源加载，又会把文件阅读变成不受控的浏览器入口。

## Evidence

- [ReaderDocument](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/ui/ReaderDocument.kt)
  用 CommonMark 解析，并维护原始代码、链接与标题目录的独立映射。
- [ReaderWebView](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/ui/ReaderWebView.kt)
  只加载 APK 中固定的阅读器 CSS / JavaScript；文件和内容访问关闭，网络请求不直接出站。
- [桌面文件读取](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/connection/DesktopTabs.kt)
  与 [SFTP 读取](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/connection/SftpClient.kt)
  分别保留自己的传输、修订与生命周期合同，不在阅读器里混合两种协议。

## Decision

- 共用正文呈现、代码复制及图片缩放组件；导航、权限、下载和文件 Tab 的所有权仍由
  相应页面维护。手机切换文件 Tab 不等价于关闭其 SSH 连接或电脑终端。
- Markdown 和图片解码离开主线程。文件预览有明确字节预算，图片先读取尺寸并采样；
  超大文件的传输使用流式下载，不通过 WebView 绕过预览预算。
- 正文中的 HTML 转义；链接由原生页面处理，相对文件链接进入文件 Tab，适用的外部
  链接交给系统处理。正文不能自行读取本地文件或加载任意网络资源。
- 复制使用原始代码，不附加行号、语言名和 Markdown fence；文件与对话共用就地
  成功反馈和可取消的恢复计时器。
- 对话增量更新保留可见消息锚点和工具折叠状态；仅在原本位于底部时跟随新消息。
  缓存只在内存中保留最近四个对话视图，每个视图受消息数和文本字节预算限制；重新
  进入时不恢复旧写入许可。文件内容跟随其页面生命周期，不另建永久文件缓存。
- 原生 WebView 使用视图边界裁剪，并在页面完成前显示覆盖式加载进度。裁剪修复的是
  AndroidView 原生合成层越界，不通过关闭整应用硬件加速改变终端渲染。

## Rejected alternatives

- 三套独立 Markdown / 复制实现：相同操作会形成不同的反馈和错误语义。
- 允许 WebView 自行下载文档资源：跳过现有 SSH / 电脑权限、修订和取消边界。
- 把全部历史和图片常驻缓存：文件及 Agent 消息的体积不受用户滚动次数限制。
- 为避免工具栏被遮挡而关闭全局硬件加速：会波及无关的终端与界面绘制。

## Consequences

当前 Markdown 范围是 CommonMark 加表格、删除线和任务列表；图片链接打开独立图片
Tab，而非让 HTML 发起不受控资源请求。数学公式和 Mermaid 需要另行定义受控渲染
资源与交互语义，不能把基础 Markdown 支持等同于所有扩展已兼容。

## Validation

雷电与真实 SSH 文件连接验证了中文 Markdown、代码与图片独立 Tab、链接跳转、复制
原始代码、图片等比缩放 / 重置和关闭文件不结束连接。WebView 冷启动期间可见加载
进度，加载完成后顶部工具栏保持可见。对话的滚动与消息收发需要各自真实场景验收，
不由文件页面的渲染验证替代。

## Supersedes

None.

## Revisit when

新增 Markdown 扩展、内嵌远端图片、离线文件缓存或流式对话渲染时，重新核对资源预算、
链接权限、正文更新锚点和页面销毁策略；保持呈现复用与传输所有权分离。
