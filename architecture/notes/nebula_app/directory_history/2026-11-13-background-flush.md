# 目录历史的后台落盘

## Status

Implemented.

## Context

`DirectoryHistory::record` 在 UI 线程同步执行完整落盘：读取整个 JSON、跨进程
合并、`age_and_limit`、`to_vec_pretty`（上限 2048 条约 279 KB）以及
`atomic_file::write` 的 `sync_all` 与写穿透 rename。每次真实 `cd` 都可能以一次
磁盘写堵塞 GPUI 帧。内存 frecency、`pending` 与返回值语义必须保持不变，
跨进程锁、读取后合并、版本/校验拒绝和失败不回退内存的既有合同也必须保留。

## Evidence

- 调用链：`gpui_shell/terminal/view.rs` 的 cwd 变化 →
  `completion::record_directory` → `DirectoryHistory::record`。
- 本机实测一次同步 `record` 约 12.4 ms；HDD 或杀毒扫描下更高，已超过一帧。
- 原 `flush` 在持有 `ServiceState` 锁时做全部磁盘 I/O；补全查询、`hint` 与
  `score` 共用该锁，因此仅把 flush 挪到线程而不缩短持锁区间仍会卡住 UI。
- 持久化文件格式、版本检查与跨进程锁路径均未改变。

## Decision

- 进程内只有一个常驻 worker 线程。`record` 同步更新内存、把观察放入
  `pending`，再通过容量为 1 的信号通道唤醒 worker；重复记录自然合并。
- worker 以首次信号为起点去抖 500 ms；每轮先从锁内取走 `pending` 快照和路径，
  释放状态锁后才获取跨进程文件锁、读盘、合并、序列化与原子写入。
- 写入成功后重新加锁，以磁盘合并结果为内存基线，并重放落盘期间新抵达且仍在
  `pending` 中的观察；内存可见性与磁盘结果不会互相覆盖。
- 抢锁失败或写盘失败时把批次放回队列头等待下一轮；写盘错误由 worker 连续计数，
  中间失败只记 `debug`。连续 3 次失败后记录一条含最近错误的 warning，关闭持久化
  并丢弃 `pending` 与迁移种子，避免按去抖节奏无限重试整库写入。
- 版本/校验错误仍只记日志并关闭持久化，不覆盖文件。纯内存实例
  （`empty()`、测试构造、worker 启动失败）不再累积 `pending`。
- 进程正常退出时 `directory_history::shutdown()` 停止并 join worker，再同步
  收尾一次；GPUI 与 legacy 壳都在事件循环结束后调用。文件格式不变。
- 去抖窗口在测试构建中缩短为 10 ms；测试轮询实际状态或使用文件锁，不假设
  固定延迟。

## Rejected alternatives

- 每次 `record` 新建线程：线程数量随 `cd` 频率无界增长，退出时也无法可靠收尾。
- 无界工作队列：worker 落后时内存与延迟都会随事件堆积。容量为 1 的唤醒信号已经
  表达“有一批待落盘记录”。
- 只在后台线程内持锁完成 I/O：磁盘延迟仍会通过共享 `ServiceState` 锁传导到
  补全查询和下一次 `record`。
- 完全去掉去抖：连续 cwd 上报会放大读写，且收益与一次合并相同。
- 写盘失败后无限重试并逐次告警：磁盘持续不可写时会每 500 ms 重写整库并刷日志；
  有界重试后关闭持久化保留了进程内可用性，代价只是本次运行不再落盘。

## Consequences

- 正常退出会排空最后一批；强制终止、或退出收尾时抢不到跨进程锁/写盘失败，都会
  丢失最后一个去抖窗口内的记录。其他进程或备份读取磁盘时，最多看到落后一个
  去抖窗口的数据。
- 同一时刻至多一个 worker 和一个待合并批次；UI 线程不再承担 JSON 与文件系统
  写入，也不在磁盘 I/O 期间持有数据库锁。
- 连续 3 次写盘失败（约 1.5 s）后持久化自动关闭，内存 frecency 与补全继续工作，
  重启后重新启用；丢弃的只是尚未落盘的那批观察，已写入文件的历史不受影响。
- 落盘期间新到的观察在重放后没有再跑 `age_and_limit`，内存库可能在一个批次内
  短暂超过 `MAX_ENTRIES`/`MAX_TOTAL_RANK`，下一次落盘收敛。

## Validation

`nebula_app/src/directory_history.rs` 的测试覆盖：`record` 后内存立即可见、
后台最终落盘、多次记录合并为一次数据库、锁竞争保留待写批次、版本不受支持时
不覆盖文件且不回退内存，以及持久化关闭后丢弃待写批次而内存记录仍可用。异步断言
使用轮询或文件锁，不依赖固定睡眠。

## Supersedes

None.

## Revisit when

落盘成本降到帧预算内、需要跨进程近实时可见性，或应用退出生命周期不再经过
`shutdown()`。
