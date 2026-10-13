package io.github.kuddev.pebrel.mobile.connection

enum class SshSessionMode(val id: String) { SHELL("shell"), TMUX("tmux"), HERDR("herdr") }

fun validRemoteSessionName(mode: SshSessionMode, name: String): Boolean =
    name.isEmpty() || (name.length <= 64 && name !in setOf(".", "..") && name.first() != '-' &&
        name.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it in "_-" || (mode == SshSessionMode.HERDR && it == '.') })

internal fun HostProfile.attachCommand(): String? {
    require(sessionMode == SshSessionMode.SHELL || validRemoteSessionName(sessionMode, sessionName))
    // 通过 SSH 的 PTY exec 请求启动客户端，不向已经运行的用户 shell 偷塞命令。
    return when (sessionMode) {
        SshSessionMode.SHELL -> null
        SshSessionMode.TMUX -> "exec tmux new-session -A -s '${sessionName.ifEmpty { "pebrel" }}'"
        SshSessionMode.HERDR -> if (sessionName.isEmpty()) "exec herdr" else "exec herdr session attach '$sessionName'"
    }
}
