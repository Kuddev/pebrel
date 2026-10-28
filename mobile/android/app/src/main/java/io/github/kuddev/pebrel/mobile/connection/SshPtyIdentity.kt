package io.github.kuddev.pebrel.mobile.connection

/** Identify the one PTY owned by this SSH connection without touching the interactive shell. */
internal object SshPtyIdentity {
    val command: String = RemoteSessions.posixWithPath(
        "printf '%s\\n' \"${'$'}${'$'}\"; ps -e -o pid= -o ppid= -o tty= -o comm=")

    private data class Process(val parent: Long, val tty: String, val command: String)
    private val servers = setOf("sshd", "sshd-session", "sshd-auth")
    private val terminal = Regex("(?:pts/[0-9]+|tty[a-zA-Z0-9]+)")

    data class Identity(val tty: String, val hasTmuxClient: Boolean)

    fun parse(output: String): String? = inspect(output)?.tty

    fun inspect(output: String): Identity? {
        if (output.length > 64 * 1024) return null
        val lines = output.lineSequence().iterator()
        val queryPid = if (lines.hasNext()) lines.next().trim().toLongOrNull() else null
        if (queryPid == null || queryPid <= 1) return null
        val processes = mutableMapOf<Long, Process>()
        for (line in lines) {
            if (line.isBlank()) continue
            val fields = line.trim().split(Regex("\\s+"), limit = 4)
            if (fields.size != 4) return null
            val pid = fields[0].toLongOrNull() ?: return null
            val parent = fields[1].toLongOrNull() ?: return null
            if (pid <= 0 || parent < 0 || processes.containsKey(pid)) return null
            processes[pid] = Process(parent, fields[2], fields[3].substringAfterLast('/'))
        }
        fun owner(start: Long): Long? {
            var pid = start
            val visited = mutableSetOf<Long>()
            repeat(32) {
                if (pid <= 1 || !visited.add(pid)) return null
                val process = processes[pid] ?: return null
                if (process.command in servers) return pid
                pid = process.parent
            }
            return null
        }
        val ssh = owner(queryPid) ?: return null
        // One app transport owns one PTY. Multiple candidates are ambiguous, never "pick first".
        val candidates = processes.filter { (pid, process) ->
            terminal.matches(process.tty) && owner(pid) == ssh
        }.values
        val tty = candidates.map { it.tty }.distinct().singleOrNull() ?: return null
        return Identity("/dev/$tty", candidates.any { it.command == "tmux" || it.command.startsWith("tmux:") })
    }
}
