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
    private var endpoint: TmuxEndpoint = TmuxEndpoint(),
) {
    private val token = UUID.randomUUID().toString().replace("-", "")
    @Volatile private var tty: String? = null
    @Volatile private var reading = false
    @Volatile private var nativeScroll = false
    private var hasMarker = false

    // No query or monitor acquisition on the per-gesture path.
    fun applicationScrolled() { nativeScroll = true }

    fun command(command: String): String {
        hasMarker = true
        return RemoteSessions.posixWithPath(
            "printf '\\036PEBREL_TTY_$token=%s\\037' \"${'$'}(tty)\"; exec ${command.removePrefix("exec ")}")
    }
    fun input(source: InputStream): InputStream = if (!hasMarker) source else PtyIdentityInput(source, token) { tty = it }

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
        clientAt(endpoint, terminal)?.let { changed(it.first); return it }
        // Missing clients also occur while a PTY is being lost. Only a still-owned shell with
        // no multiplexer client proves an intentional detach; an absent/ambiguous PTY is unknown.
        val identity = query(SshPtyIdentity.command).takeIf { it.getInt("status") == 0 }
            ?.let { SshPtyIdentity.inspect(it.getString("stdout")) } ?: return null
        if (identity.tty != terminal) return null
        if (!identity.hasTmuxClient) {
            reading = false
            nativeScroll = false
            changed(null)
            return null
        }
        val discovered = query(TmuxEndpoint.discoveryCommand)
        if (discovered.getInt("status") != 0) return null
        val clients = TmuxEndpoint.parse(discovered.getString("stdout")).filter { it != endpoint }
            .mapNotNull { candidate -> clientAt(candidate, terminal) }
        // A reused tty or ambiguous server inventory must never select another task.
        return clients.singleOrNull()?.also {
            endpoint = it.first.session.endpoint
            changed(it.first)
        }
    }

    private fun clientAt(candidate: TmuxEndpoint, terminal: String): Pair<RemoteAttachment, String>? {
        val response = query(RemoteSessions.posixWithPath(
            "${candidate.command()} list-clients -F '#{client_tty}\t#{session_id}\t#{session_name}\t#{session_created}:#{pid}\t#{window_id}\t#{window_name}\t#{pane_id}\t#{pane_mode}'"))
        if (response.getInt("status") != 0) return null
        return parseClient(response.getString("stdout"), terminal)?.let { (attachment, mode) ->
            attachment.copy(session = attachment.session.copy(endpoint = candidate)) to mode
        }
    }

    @Synchronized
    fun scroll(lines: Int): Boolean {
        if (lines == 0) return true
        val (attachment, mode) = current() ?: return false
        if (mode.isNotEmpty() && mode != "copy-mode") return true
        if (lines > 0 && mode != "copy-mode") return true
        val target = target(attachment)
        val clientCommand = attachment.session.endpoint.command()
        val enter = if (mode == "copy-mode") "" else "$clientCommand copy-mode -t '$target' && "
        val action = if (lines < 0) "scroll-up" else "scroll-down"
        execute(attachment, enter + "$clientCommand send-keys -X -N ${kotlin.math.abs(lines.coerceIn(-256, 256))} -t '$target' $action")
        reading = true
        return true
    }

    @Synchronized
    fun returnToInput() {
        if (!reading && !nativeScroll) return
        val client = current()
        if (client == null) {
            if (!reading) { nativeScroll = false; return }
            throw IOException("tmux_client_unavailable")
        }
        val (attachment, mode) = client
        if (mode == "copy-mode") execute(attachment, "${attachment.session.endpoint.command()} send-keys -X -t '${target(attachment)}' cancel")
        reading = false
        nativeScroll = false
    }

    private fun execute(attachment: RemoteAttachment, command: String) {
        val session = attachment.session
        // Reused remote multiplexer IDs after a server restart must never target another task.
        val guard = "[ \"${'$'}(${session.endpoint.command()} display-message -p -t '${session.id}' '#{session_created}:#{pid}')\" = '${session.identity}' ] && "
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
