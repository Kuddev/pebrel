# Windows 产品内存压力测试

`windows_memory_stress.py` 启动真实 GPUI 产品，通过真实 PTY 写入持续输出，
再反复创建和关闭标签。它检查输出结束后内存、线程和句柄能否保持稳定。
测试使用独立配置、工作目录和临时目录；每次输出目录必须是新目录。

需要 Windows 原生 Python 3.11+，不需要第三方 Python 包。先构建真实产品：

```powershell
cargo build --release --locked -p nebula --bin pebrel --features gpui-shell
```

确认 `pebrel.exe` 同目录存在产品附带的 `conpty.dll` 和 `OpenConsole.exe`。
后面的命令在仓库根目录运行；构建及测试输出都应放在当前环境允许的路径内。

## 短回归与长测

以下预算用于本机 Windows 11、144 DPI、1280 × 900 外窗、默认 10000 行历史、
Maple Mono Normal NF CN 16 px 的回归条件。首次校准可不传预算，仅观察数据；
未配置预算的报告会明确标为观察运行。预算一旦选定，应在前后对照中保持一致，
超标时先定位分配和资源生命周期。

```powershell
python scripts/windows_memory_stress.py `
  --app target/release/pebrel.exe `
  --output tmp/memory-short-01 `
  --cycles 8 --tabs 4 --payload-mib 11 `
  --max-private-commit-mib 256 --max-growth-mib 16 `
  --max-handle-growth 8 --max-thread-growth 4

python scripts/windows_memory_stress.py `
  --app target/release/pebrel.exe `
  --output tmp/memory-long-01 `
  --cycles 32 --tabs 4 --payload-mib 64 `
  --max-private-commit-mib 256 --max-growth-mib 16 `
  --max-handle-growth 8 --max-thread-growth 4
```

长测包含约 2 GiB 混合 ASCII、CJK、Nerd Font 和 ANSI 彩色输出，以及
128 次标签关闭。每一轮输出后还写入 240 次交替屏幕重绘。
语料按完整块向上取整，报告记录实际字节数。它不是固定帧率动画或显示 FPS 测试。

程序会固定窗口大小，并从终端内读取真实行列数。比较两个程序时必须检查报告中的
行列数相同，仅设置相同像素尺寸并不足够。测试期间应保留正常显示的窗口，避免
输入、切换配置或调整窗口尺寸。将编译和其他高负载工作与正式测量分开。

## 测量口径与通过条件

| 数据 | 含义 |
| --- | --- |
| `private_commit_bytes` | Pebrel 自己的私有提交量，是本脚本内存预算的依据 |
| `private_working_set_bytes` | Pebrel 当前驻留在 RAM 中的私有页面 |
| `working_set_bytes` | 总工作集，包含驻留的共享页面，不能再与私有工作集相加 |
| `cpu_seconds` | Pebrel 累积的用户态和内核态 CPU 时间 |
| `handles` / `threads` | Pebrel 的句柄和线程数；线程在阶段检查点读取 |

每 100 ms 采样一次。完成输出并等待稳定、或关闭新增标签并恢复单标签后，
用该阶段最后五个内存样本的中位数比较。第一轮负责预热，后续每一轮都与它比较，
取最大增长量；不能让最后一次回收掩盖前面某一轮的残留增长。

脚本同时检查全部采样中的私有提交峰值、预热后的提交增长、句柄增长和线程增长。
输出必须在实际终端里出现完成标记；所有阶段必须齐全。测试窗口必须恢复到一个
标签并保持原几何状态，进程最终正常退出。任一预算超标或阶段失败均返回非零退出码。
关闭确认失败时保留现场进程树，不自动确认关闭来绕过产品的工作保护。

清理先对本次创建的窗口发送关闭请求。若失败，仅终止脚本保有句柄的那个测试进程，
并把该次运行记为失败。不会按进程名称批量结束实例。脚本没有设置进程内存上限、
调用工作集裁剪或生成完整内存转储。

## 证据文件

- `instance.json`：PID、进程创建时间、EXE SHA256、脚本 SHA256、Windows/Python
  版本和逻辑处理器数；可通过 `--source-manifest <文件>` 同时记录源码清单的路径和 SHA256。
- `report.json`：实际窗口/网格、每轮输出量、提交峰值、残留增长及预算结果。
- `samples.json` / `checkpoints.json`：原始计数，即使测试中断也保存。
  检查点还记录系统总物理内存、可用物理内存和内存负载，帮助识别页面驻留差异。
- `failure.json` / `failure-context.json`：失败原因、所处阶段及可读到的测试实例进程树。
- `cleanup.json`：是否正常退出，是否发生强制清理。
- `work/*-<轮次>.json`：终端内写入程序记录的实际输出量、行列数和写入耗时。

`drain_seconds` 包含 PTY 背压等待，不代表输入延迟或屏幕真正呈现的帧率。
100 ms 采样可能错过更短的峰值。计数不含 shell、输出程序或 WSL 虚拟机；
如需比较整个终端产品的进程组，应另行使用一致的进程归属和计数口径。
工作集还受页面驻留、其他应用和驱动影响，不能凭它下降就宣称减少了分配。
上述测试预算也不是应用在任意硬件、任意标签数下的运行时硬上限。

## 相关回归与实现依据

```powershell
python -m unittest scripts.tests.test_windows_memory_stress
cargo test --locked -p nebula_terminal --test windows_pty_lifecycle -- --nocapture
cargo test --locked -p nebula --bin pebrel --features gpui-shell process_tree:: -- --test-threads=1
```

本地终端输出沿用现有有界 PTY 缓冲和有限历史网格。此次回收修复关闭了创建
ConPTY 时遗留的调用方管道副本，让 EOF 能结束读取/排空线程；shell、主线程、
属性表和旁加载 DLL 使用各自的所有权释放。等待回调完成或取消后才释放其上下文，
避免把内存回收变成回调访问已释放数据。

字体修复由精确固定的 GPUI 分支提供：DirectWrite 持有字体数据的 COM 所有者，
静态字体直接使用程序映像中的字节，导入字体保留原 Vec，直到最后一个原生消费者
释放。原生内容、指针、引用寿命测试与真实窗口字体比对属于独立验收项。

Windows 会保留已经退出的父进程 PID；号码复用后，仅按 PID 连边会把无关进程
误接到新标签上。平台适配层现在一次读取进程创建时间，公共进程树规则断开
“父进程比子进程还年轻”的关系。缺失创建时间时保留关系，真实忙碌子进程仍受保护。
这使用已存在的 Windows 绑定，无额外后台轮询或持久化格式。

生命周期回归使用产品附带的 OpenConsole。Windows 11 build 22631 的系统内置
ConPTY 在独立最小探针中仍出现每次创建/关闭保留一个宿主句柄的问题；本次修改
没有修复该系统实现，也没有放宽产品回归预算去吸收它。
