package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import java.io.IOException
import java.io.InputStream
import java.util.UUID

/** Typed remote multiplexer commands use an independent SSH exec channel, never the user's interactive input. */
internal class TmuxSessionControl(
    private val query: (String) -> JSONObject,
    private val changed: (RemoteAttachment?) -> Unit,
    private val detectShell: Boolean = false,
) {
    private val token = UUID.randomUUID().toString().replace("-", "")
    @Volatile private var tty: String? = null
    @Volatile private var reading = false

    fun command(command: String): String = RemoteSessions.posixWithPath(
        "printf '\\036PEBREL_TTY_$token=%s\\037' \"${'$'}(tty)\"; exec ${command.removePrefix("exec ")}")
    fun input(source: InputStream): InputStream = if (detectShell) source else PtyIdentityInput(source, token) { tty = it }

    @Synchronized
    fun identifyShellTerminal() {
        if (!detectShell || tty != null) return
        val response = query(SshPtyIdentity.command)
        if (response.getInt("status") == 0) {
            tty = SshPtyIdentity.parse(response.getString("stdout"))
            if (tty != null) checkpoint()
        }
    }

    @Synchronized
    fun checkpoint(): RemoteAttachment? = current()?.first

    private fun current(): Pair<RemoteAttachment, String>? {
        val terminal = tty ?: return null
        val response = query(RemoteSessions.posixWithPath(
            "tmux -u list-clients -F '#{client_tty}\t#{session_id}\t#{session_name}\t#{session_created}:#{pid}\t#{window_id}\t#{window_name}\t#{pane_id}\t#{pane_mode}'"))
        if (response.getInt("status") != 0) return null
        val output = response.getString("stdout")
        val client = parseClient(output, terminal)
        if (client != null) changed(client.first)
        else if (detectShell && output.lineSequence().none { it.startsWith("$terminal\t") }) {
            // An intentional detach returns to the shell; do not revive that old remote multiplexer target later.
            reading = false
            changed(null)
        }
        return client
    }

    @Synchronized
    fun scroll(lines: Int): Boolean {
        if (lines == 0) return true
        val (attachment, mode) = current() ?: return !detectShell
        if (mode.isNotEmpty() && mode != "copy-mode") return true
        if (lines > 0 && mode != "copy-mode") return true
        val target = target(attachment)
        val enter = if (mode == "copy-mode") "" else "tmux copy-mode -t '$target' && "
        val action = if (lines < 0) "scroll-up" else "scroll-down"
        execute(attachment, enter + "tmux send-keys -X -N ${kotlin.math.abs(lines.coerceIn(-32, 32))} -t '$target' $action")
        reading = true
        return true
    }

    @Synchronized
    fun returnToInput() {
        if (!reading) return
        val client = current()
        if (client == null) {
            if (!reading) return // The automatic shell controller observed an intentional detach.
            throw IOException("tmux_client_unavailable")
        }
        val (attachment, mode) = client
        if (mode == "copy-mode") execute(attachment, "tmux send-keys -X -t '${target(attachment)}' cancel")
        reading = false
    }

    private fun execute(attachment: RemoteAttachment, command: String) {
        val session = attachment.session
        // Reused remote multiplexer IDs after a server restart must never target another task.
        val guard = "[ \"${'$'}(tmux display-message -p -t '${session.id}' '#{session_created}:#{pid}')\" = '${session.identity}' ] && "
        if (query(RemoteSessions.posixWithPath(guard + command)).getInt("status") != 0) throw IOException("tmux_history_failed")
    }

    private fun target(attachment: RemoteAttachment) = "${attachment.session.id}:${attachment.window!!.id}.${attachment.window.pane}"

    companion object {
        internal fun parseClient(output: String, tty: String): Pair<RemoteAttachment, String>? = output.lineSequence().mapNotNull { line ->
            val fields = line.trimEnd('\r').split('\t')
            if (fields.size != 8 || fields[0] != tty || !fields[1].matches(Regex("\\$[0-9]+")) ||
                !fields[3].matches(Regex("[0-9]+:[0-9]+")) || !fields[4].matches(Regex("@[0-9]+")) ||
                !fields[6].matches(Regex("%[0-9]+")) || fields.any { it.length > 256 || it.any(Char::isISOControl) }) null
            else RemoteAttachment(RemoteSession("tmux", fields[1], fields[2], fields[3]),
                RemoteWindow(fields[4], fields[5], pane = fields[6])) to fields[7]
        }.singleOrNull()
    }
}
