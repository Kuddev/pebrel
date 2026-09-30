# SFTP 子通道与传输取消的所有权

## Status

Implemented.

## Context

SSH 终端需要同时浏览目录和传输文件。如果把文件请求放进终端命令循环，文件请求
等待服务器时会阻塞按键和输出；若每次打开文件树都新建 SSH 连接，又会重复认证并
分裂主机指纹和连接生命周期。

## Evidence

- [SSH 传输循环](../../../../mobile/ssh/src/transport.rs)已经持有认证后的
  `russh::client::Handle`，可以从同一连接打开独立 subsystem channel。
- [SFTP 适配](../../../../mobile/ssh/src/sftp.rs)使用 `russh-sftp` 的 v3 协议实现，
  自己只维护目录游标、传输句柄、修订检查和上传临时文件。
- [Android 客户端](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/connection/SftpClient.kt)
  在 I/O 调度器调用 JNI；协程取消不代表已经发出的原生请求尚未创建句柄。

## Decision

- 复用 SSH 认证与主机指纹，按需建立 SFTP 子通道。终端接收、发送和 SFTP 分别拥有
  独立 future，外层统一跟随 SSH 连接结束。
- 文件请求顺序处理，分块为 32 KiB；目录每页最多 256 条，原生层只持有一个目录
  游标和最多四个传输句柄。新目录请求释放旧游标，空闲句柄由周期清理回收。
- 原生句柄 ID 单调递增，文件通道重开后不复用；旧取消操作不会关闭新传输。
- JNI 返回前的取消仍取回已创建的句柄，并在 `NonCancellable` 清理段归还。UI 页面
  释放时先取消分页和上传，再关闭自己持有的游标。
- 上传写入同目录的独占临时文件，核验实际大小后执行不覆盖目标的 SFTP v3 rename。
  取消尽力关闭句柄并删除本次临时文件，不覆盖已有目标，不重放未知结果的写入。
- 预览限制为 16 MiB；下载通过 Android 文件选择器提供的流分块写入，不先把大文件
  整体载入 JVM。文件通道超时只回收文件通道，不主动结束仍可用的 shell。

## Rejected alternatives

- 另开一条 SSH 连接处理每次文件操作：重复认证、重复凭据状态，且关闭语义不一致。
- 在 PTY 上执行 `ls` / `cat` 并解析输出：文件名、编码、终端控制符和 shell 配置会改变协议。
- 直接写目标文件或使用覆盖式 rename：失败、取消与目标冲突会损坏已有内容。
- 用固定句柄槽位作为公开 ID：通道重建后的迟到取消可能命中新操作。

## Consequences

同一主机的 shell 与文件操作共享 SSH 连接，但不共享终端字节流。目录游标有空闲
期限，过期后需就地刷新；该恢复操作不是重新连接 SSH。断网后服务端遗留的临时文件
不保证立即清理，界面不得把未确认的传输显示为成功。

## Validation

原生测试覆盖请求参数、句柄和协议边界。Android `SshIntegrationTest` 通过真实
OpenSSH 验证目录分页、UTF-8 文件名、分块往返、取消、目标不覆盖、重命名和删除，
并在传输期间验证 shell 仍可收发。手机 UI 另行验证系统文件选择器与文件 Tab 操作。

## Supersedes

None.

## Revisit when

新增多文件队列、并发目录窗口、断点续传或覆盖上传时，重新定义传输所有权与用户确认
语义；保持 SSH 认证的单一来源，并用真实终端收发证明文件操作未阻塞 PTY。
