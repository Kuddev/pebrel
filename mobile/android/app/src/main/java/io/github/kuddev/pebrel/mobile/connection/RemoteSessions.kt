package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONArray
import org.json.JSONObject

data class RemoteWindow(val id: String, val label: String, val workspace: String = "", val state: String = "")
data class RemoteSession(
    val kind: String, val id: String, val name: String, val identity: String = "",
    val attached: Boolean = false, val windows: List<RemoteWindow> = emptyList(),
    val windowsHost: Boolean = false,
)
data class RemoteInventory(val os: String, val sessions: List<RemoteSession>, val warnings: Set<String> = emptySet())
data class RemoteAttachment(val session: RemoteSession, val window: RemoteWindow? = null) {
    val title get() = window?.label ?: session.name
}

/** Discovery is read-only, bounded by the native query channel, and never enters a PTY. */
object RemoteSessions {
    private const val path = "PATH=\"${'$'}PATH:/opt/homebrew/bin:/usr/local/bin:${'$'}HOME/.local/bin:${'$'}HOME/.cargo/bin\"; export PATH; "
    private fun posix(script: String) = "sh -c '${script.replace("'", "'\"'\"'")}'"
    internal val discoveryCommand = posix(path + """
        printf '\036os\n'; uname -s 2>/dev/null
        printf '\036distro\n'; cat /etc/os-release 2>/dev/null
        printf '\036tmux\n'
        if command -v tmux >/dev/null 2>&1; then
          tmux list-sessions -F '#{session_id}\t#{session_name}\t#{session_attached}\t#{session_created}:#{pid}' 2>/dev/null
          printf '\036windows\n'
          tmux list-windows -a -F '#{session_id}\t#{window_id}\t#{window_index}\t#{window_name}' 2>/dev/null
        fi
        printf '\036herdr\n'
        if command -v herdr >/dev/null 2>&1; then herdr session list --json 2>/dev/null; fi
        printf '\036end\n'
    """.trimIndent().replace("\\t", "\t"))

    // 编码整个脚本，避免 cmd/PowerShell 两次解释会话名里的引号或元字符。
    internal fun powershell(script: String): String = "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand " +
        java.util.Base64.getEncoder().encodeToString((
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); " +
            "${'$'}env:PATH += ';' + ${'$'}HOME + '\\.cargo\\bin;' + ${'$'}HOME + '\\.local\\bin'; " + script
        ).toByteArray(Charsets.UTF_16LE))

    fun discover(connection: SshConnection): RemoteInventory {
        val response = connection.query(discoveryCommand)
        val body = response.optString("stdout")
        if (response.getInt("status") == 0 && body.contains("\u001eos\n")) return parse(body)
        // Windows OpenSSH 的默认 shell 可能是 cmd 或 PowerShell，不能把 POSIX 失败当作 Linux。
        val windows = connection.query(powershell(
            "[Console]::Write([char]30 + 'os' + [char]10 + 'windows' + [char]10 + [char]30 + 'herdr' + [char]10); " +
            "if (Get-Command herdr -ErrorAction SilentlyContinue) { & herdr session list --json; if (${ '$' }LASTEXITCODE -ne 0) { exit ${ '$' }LASTEXITCODE } }; " +
            "[Console]::Write([char]30 + 'end' + [char]10); exit 0"))
        check(windows.getInt("status") == 0)
        return parse(windows.getString("stdout")).also { check(it.os == "windows") }
    }

    private fun sections(text: String): Map<String, String> = text.split('\u001e').drop(1).associate {
        it.substringBefore('\n').trimEnd('\r') to it.substringAfter('\n', "").trimEnd('\r', '\n')
    }
    private fun valid(value: String) = value.isNotBlank() && value.length <= 256 && value.none(Char::isISOControl)
    private fun quote(value: String): String {
        require(valid(value))
        return "'${value.replace("'", "'\"'\"'")}'"
    }

    internal fun parse(text: String): RemoteInventory {
        require(text.toByteArray(Charsets.UTF_8).size <= 64 * 1024)
        val data = sections(text)
        val system = data["os"].orEmpty().trim().lowercase(java.util.Locale.ROOT)
        val distro = data["distro"].orEmpty().lineSequence().firstOrNull { it.startsWith("ID=") }
            ?.substringAfter('=')?.trim('"', '\'')
        val os = if (system == "linux" && distro != null) desktopOsIcon(distro).takeUnless { it == "term" } ?: "linux"
            else desktopOsIcon(system)
        val windows = data["windows"].orEmpty().lineSequence().mapNotNull { line ->
            val fields = line.split('\t')
            if (fields.size != 4 || !fields[0].matches(Regex("\\$[0-9]+")) || !fields[1].matches(Regex("@[0-9]+")) || !valid(fields[3])) null
            else fields[0] to RemoteWindow(fields[1], "${fields[2]} · ${fields[3]}")
        }.take(256).toList().groupBy({ it.first }, { it.second })
        val sessions = data["tmux"].orEmpty().lineSequence().mapNotNull { line ->
            val fields = line.split('\t')
            if (fields.size != 4 || !fields[0].matches(Regex("\\$[0-9]+")) || !valid(fields[1]) || !fields[3].matches(Regex("[0-9]+:[0-9]+"))) null
            else RemoteSession("tmux", fields[0], fields[1], fields[3], (fields[2].toIntOrNull() ?: 0) > 0, windows[fields[0]].orEmpty())
        }.take(128).toMutableList()
        val warnings = mutableSetOf<String>()
        data["herdr"]?.takeIf { it.isNotBlank() }?.let { body ->
            runCatching {
                val rows = JSONObject(body).getJSONArray("sessions")
                require(rows.length() <= 128)
                for (i in 0 until rows.length()) {
                    val row = rows.getJSONObject(i)
                    val name = row.getString("name")
                    if (row.optBoolean("running") && valid(name) && validRemoteSessionName(SshSessionMode.HERDR, name))
                        sessions += RemoteSession("herdr", name, name, windowsHost = os == "windows")
                }
            }.onFailure { warnings += "herdr" }
        }
        return RemoteInventory(os, sessions.distinctBy { it.kind to it.id }, warnings)
    }

    fun windows(connection: SshConnection, session: RemoteSession): List<RemoteWindow> {
        if (session.kind != "herdr") return session.windows
        val prefix = "herdr --session ${quote(session.id)} "
        val command = if (session.windowsHost) powershell(
            "[Console]::Write([char]30 + 'workspaces' + [char]10); & herdr --session ${psQuote(session.id)} workspace list; " +
            "if (${ '$' }LASTEXITCODE -ne 0) { exit ${ '$' }LASTEXITCODE }; " +
            "[Console]::Write([char]30 + 'tabs' + [char]10); & herdr --session ${psQuote(session.id)} tab list; exit ${ '$' }LASTEXITCODE")
        else posix(path + "printf '\\036workspaces\\n'; ${prefix}workspace list && " +
            "{ printf '\\036tabs\\n'; ${prefix}tab list; }")
        val result = connection.query(command)
        check(result.getInt("status") == 0)
        val data = sections(result.getString("stdout"))
        fun rows(key: String): JSONArray {
            val response = JSONObject(checkNotNull(data[key]))
            check(response.optBoolean("ok", true))
            return (response.optJSONObject("result") ?: response).getJSONArray(key).also { require(it.length() <= 256) }
        }
        val workspaces = rows("workspaces")
        val labels = (0 until workspaces.length()).associate { i -> workspaces.getJSONObject(i).let {
            it.getString("workspace_id") to it.optString("label").ifBlank { it.getString("workspace_id") }
        } }
        val tabs = rows("tabs")
        return (0 until tabs.length()).mapNotNull { i -> tabs.getJSONObject(i).let { tab ->
            val id = tab.getString("tab_id")
            val workspace = tab.getString("workspace_id")
            if (!valid(id) || !valid(workspace)) null else RemoteWindow(id,
                "${labels[workspace] ?: workspace} · ${tab.optString("label").ifBlank { id }}", workspace,
                tab.optString("agent_status"))
        } }
    }

    private fun psQuote(value: String): String {
        require(valid(value))
        return "'${value.replace("'", "''")}'"
    }

    fun attachCommand(attachment: RemoteAttachment): String {
        val session = attachment.session
        val window = attachment.window
        if (session.windowsHost) {
            require(session.kind == "herdr" && validRemoteSessionName(SshSessionMode.HERDR, session.id))
            val selected = window?.let { "& herdr --session ${psQuote(session.id)} tab focus ${psQuote(it.id)}; " +
                "if (${ '$' }LASTEXITCODE -ne 0) { exit ${ '$' }LASTEXITCODE }; " }.orEmpty()
            return powershell(selected + "& herdr session attach ${psQuote(session.id)}; exit ${ '$' }LASTEXITCODE")
        }
        return posix(path + when (session.kind) {
            "tmux" -> {
                require(session.id.matches(Regex("\\$[0-9]+")) && session.identity.matches(Regex("[0-9]+:[0-9]+")))
                val selected = window?.let {
                    require(it.id.matches(Regex("@[0-9]+")))
                    "tmux select-window -t ${quote("${session.id}:${it.id}")} && "
                }.orEmpty()
                // 不按可重命名的标题匹配；服务器重启后不能误接到复用的 session ID。
                "[ \"${'$'}(tmux display-message -p -t ${quote(session.id)} '#{session_created}:#{pid}')\" = ${quote(session.identity)} ] && " +
                    selected + "exec tmux attach-session -t ${quote(session.id)}"
            }
            "herdr" -> {
                require(validRemoteSessionName(SshSessionMode.HERDR, session.id))
                val selected = window?.let { "herdr --session ${quote(session.id)} tab focus ${quote(it.id)} && " }.orEmpty()
                selected + "exec herdr session attach ${quote(session.id)}"
            }
            else -> error("unsupported_session")
        })
    }
}
