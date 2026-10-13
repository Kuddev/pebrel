# 页面退出动画中的终端输入所有权

## Status

Implemented; validation is recorded below.

## Context

文件页的系统返回先取消多选，否则回到终端页面；目录层级由 `..` 导航，
不再作为页面返回历史。页面动画会短暂保留已经退出的终端视图。

## Evidence

雷电 API 28 上复现：点击终端后马上打开 SFTP，`dumpsys input_method`
在文件页报告 `mInputShown=true`，第一次返回只关闭键盘，多选仍保留。
`GhosttyView.onSingleTapConfirmed` 在双击判定结束后才请求焦点和键盘；
退出动画中的旧视图仍然 attached，单独检查 attached 状态不足以判断输入所有权。

## Decision

[MainActivity](../../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/MainActivity.kt)
将当前路由是否仍为终端传入
[LocalTerminalScreen](../../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/ui/TerminalScreen.kt)。
终端的实际直接输入状态是用户选择与页面 active 状态的交集。
离开页面即复用 `GhosttyView.directInput=false` 释放焦点、隐藏键盘；
已排队的单击或键盘请求也通过原有 directInput 检查停止。

## Rejected alternatives

- 在文件页延迟隐藏键盘：无法证明延时晚于旧终端回调，还可能关闭用户刚打开的路径输入框。
- 让返回键额外执行一次：掩盖错误的焦点归属，破坏系统 IME 与返回语义。
- 把用户的直接输入偏好改为关闭：把临时页面生命周期泄漏进持久化设置。

## Consequences

退出视图可以继续绘制动画，但不再拥有终端输入焦点；返回终端时恢复原本的
直接输入模式。没有新增延时、持久化字段或第二套键盘管理器。

## Validation

[SftpBrowserTest](../../../../../mobile/android/app/src/test/java/io/github/kuddev/pebrel/mobile/ui/SftpBrowserTest.kt)
覆盖旧视图尚未销毁时停止直接输入与释放焦点，以及再次 active 后恢复原设置。
目录导航、多选取消和页面返回使用同一组真实 Compose 布局测试。
雷电 API 28 复测「点击终端后立即打开 SFTP」，文件页保持 `mInputShown=false`；
紧接着的多选只需一次系统返回即可取消，不再先消耗一次返回关闭旧键盘。

## Supersedes

None.

## Revisit when

页面导航不再保留退出动画视图，或终端输入会话改为由统一导航生命周期直接管理时，
重新评估 active 参数；始终保留延迟单击不向已离开页面弹键盘的行为合同。
