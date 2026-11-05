package io.github.kuddev.pebrel.mobile.connection

import android.util.Base64
import io.github.kuddev.pebrel.terminal.TerminalSession
import io.github.kuddev.pebrel.terminal.TerminalShortcut
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.util.zip.DeflaterOutputStream

/** Names shown for the Agents an SSH server can run. */
fun sshAgentName(kind: String): String = when (kind) {
    "claude" -> "Claude Code"
    "codex" -> "Codex"
    "pi" -> "Pi"
    "antigravity" -> "Antigravity"
    else -> kind
}

/**
 * Runs the bundled transcript reader on the server over an exec channel. The script is installed once under
 * `~/.cache/pebrel`, named by its hash, because a single exec command is limited to 8 KiB.
 */
internal class SshAgentChat(
    private val script: ByteArray,
    private val query: suspend (String) -> JSONObject,
    /** The PTY to inspect when a multiplexer client on this connection shows another pane; null for its own PTY. */
    private val paneTerminal: suspend () -> String? = { null },
) {
    private val file = "\$HOME/.cache/pebrel/ssh-agent-chat-" +
        MessageDigest.getInstance("SHA-256").digest(script).take(6).joinToString("") { "%02x".format(it) } + ".py"

    suspend fun call(request: JSONObject): JSONObject {
        paneTerminal()?.let { request.put("tty", it) }
        val argument = Base64.encodeToString(request.toString().toByteArray(), Base64.NO_WRAP)
        val command = RemoteSessions.posixWithPath(
            "f=\"$file\"; [ -f \"\$f\" ] || exit 86; command -v python3 >/dev/null 2>&1 || exit 87; " +
                "exec python3 \"\$f\" '$argument'")
        var reply = query(command)
        if (reply.getInt("status") == 86) {
            install()
            reply = query(command)
        }
        when (reply.getInt("status")) {
            0 -> Unit
            87 -> throw DesktopRpcFailure("python_missing")
            else -> throw DesktopRpcFailure("conversation_unavailable")
        }
        val line = reply.getString("stdout").lineSequence().lastOrNull { it.startsWith(PREFIX) }
            ?: throw DesktopRpcFailure("conversation_unavailable")
        val result = JSONObject(line.removePrefix(PREFIX))
        if (result.optInt("version") != 1) throw DesktopRpcFailure("conversation_unavailable")
        result.optString("error").takeIf { it.isNotEmpty() }?.let { throw DesktopRpcFailure(it) }
        return result
    }

    private suspend fun install() {
        val packed = ByteArrayOutputStream().also { out -> DeflaterOutputStream(out).use { it.write(script) } }.toByteArray()
        val encoded = Base64.encodeToString(packed, Base64.NO_WRAP)
        val staged = "$file.b64"
        check(query(RemoteSessions.posixWithPath("umask 077; mkdir -p \"\$HOME/.cache/pebrel\" && : > \"$staged\""))
            .getInt("status") == 0)
        for (chunk in encoded.chunked(6000)) {
            check(query(RemoteSessions.posixWithPath("printf %s '$chunk' >> \"$staged\"")).getInt("status") == 0)
        }
        val unpack = "import base64,sys,zlib;open(sys.argv[2],'wb').write(zlib.decompress(base64.b64decode(open(sys.argv[1]).read())))"
        val status = query(RemoteSessions.posixWithPath(
            "command -v python3 >/dev/null 2>&1 || exit 87; python3 -c \"$unpack\" \"$staged\" \"$file.tmp\" && " +
                "mv \"$file.tmp\" \"$file\"; s=\$?; rm -f \"$staged\" \"$file.tmp\"; exit \$s")).getInt("status")
        if (status == 87) throw DesktopRpcFailure("python_missing")
        check(status == 0)
    }

    private companion object {
        const val PREFIX = "PEBREL_SSH_CHAT="
    }
}

/** A selection menu the Agent draws in the terminal: numbered options, one of them marked as current. */
internal object TerminalPrompt {
    private val border = Regex("^[\\s│┃|╭╮╰╯─]+|[\\s│┃|╭╮╰╯─]+$")
    private val option = Regex("^([❯›>▶]\\s*)?(\\d{1,2})[.)]\\s+(.+)$")
    private val running = Regex("(?i)\\b(?:esc|ctrl\\+c) to interrupt\\b")

    fun isRunning(screen: String): Boolean = screen.lineSequence().toList().takeLast(12).any(running::containsMatchIn)

    fun parse(screen: String): ConversationPrompt? {
        val lines = screen.lines().takeLast(30).map { it.replace(border, "") }
        val last = lines.indexOfLast { option.matches(it) }
        if (last < 0) return null
        // Walk up through the menu. Descriptions may sit between options; anything else ends it.
        val found = mutableListOf<Pair<Int, MatchResult>>()
        var index = last
        var gap = 0
        while (index >= 0 && gap <= 2) {
            val match = option.matchEntire(lines[index])
            if (match != null) { found.add(0, index to match); gap = 0 } else gap++
            if (match?.groupValues?.get(2) == "1") break
            index--
        }
        val numbers = found.map { it.second.groupValues[2].toInt() }
        if (numbers.size < 2 || numbers != (1..numbers.size).toList()) return null
        val selected = found.indexOfFirst { it.second.groupValues[1].isNotEmpty() }
        if (selected < 0) return null
        // A question longer than the terminal is wrapped across rows; full-width rows continue without a space.
        val width = screen.lines().maxOfOrNull { it.trimEnd().length } ?: 0
        val above = lines.subList(0, found.first().first).dropLastWhile { it.isBlank() }
        val rows = above.takeLastWhile { it.isNotBlank() }.takeLast(4)
        val question = rows.foldIndexed("") { index, text, row ->
            val joiner = if (index == 0 || rows[index - 1].length >= width - 1) "" else " "
            text + joiner + row.trim()
        }
        val options = found.map { it.second.groupValues[3].trim() }
        val id = MessageDigest.getInstance("SHA-256").digest((question + "\u0000" + options.joinToString("\u0000")).toByteArray())
            .joinToString("") { "%02x".format(it) }
        return ConversationPrompt(id, question, options, selected, binary = false)
    }
}

/** An Agent running in this app's own SSH terminal. Reads come from its transcript; input goes to the terminal. */
internal class SshConversation(
    private val kind: String,
    private val chat: SshAgentChat,
    private val terminal: () -> TerminalSession?,
) : ConversationSource {
    private val current = MutableStateFlow(ConversationPage(ConversationIdentity(kind, "", 1)))
    override val state: StateFlow<ConversationPage> = current.asStateFlow()
    private val reads = Mutex()
    private val input = Mutex()

    private suspend fun screen(): String = terminal()?.screenText().orEmpty()

    override suspend fun refresh(older: Boolean): Boolean = reads.withLock {
        val old = current.value
        if (older && old.before == null) return@withLock false
        val request = JSONObject().put("mode", "read")
        if (older) request.put("before", old.before) else old.revision?.let { request.put("revision", it) }
        val response = chat.call(request)
        val agent = response.optString("agent")
        if (agent != kind) throw DesktopRpcFailure("conversation_ended")
        val session = response.optString("session")
        // Another transcript (a new or resumed session) replaces the page instead of merging into it.
        val base = if (session != old.identity.session) ConversationPage(ConversationIdentity(kind, session, 1)) else old
        val unchanged = response.optBoolean("unchanged") && base === old
        val received = if (unchanged) emptyList() else response.optJSONArray("messages")?.let { rows ->
            (0 until rows.length()).map { i -> rows.getJSONObject(i).let { row ->
                ConversationMessage(row.getString("id"), row.getString("role"), row.getString("text"),
                    if (row.isNull("name")) null else row.getString("name"),
                    if (row.isNull("detail")) null else row.getString("detail"), row.getBoolean("complete"),
                    row.getBoolean("truncated"), row.optBoolean("error"))
            } }
        }.orEmpty()
        val messages = when {
            unchanged -> base.messages
            older -> (received + base.messages).distinctBy { it.id }
            received.isEmpty() -> if (base === old) base.messages else emptyList()
            else -> base.messages.takeWhile { offset(it.id) < offset(received.first().id) } + received
        }
        val text = screen()
        val prompt = TerminalPrompt.parse(text)
        val next = base.copy(messages = messages,
            revision = if (older) base.revision else response.optString("revision").ifEmpty { null },
            before = when {
                unchanged -> base.before
                older || !base.loaded -> if (response.isNull("before")) null else response.optLong("before")
                else -> base.before
            },
            cwd = response.optString("cwd", base.cwd), loaded = true, prompt = prompt,
            canSend = terminal()?.isReady == true && prompt == null,
            state = when {
                prompt != null -> "waiting_input"
                TerminalPrompt.isRunning(text) -> "running"
                else -> "idle"
            },
            truncated = base.truncated || response.optBoolean("truncated"))
        current.value = next
        next != old
    }

    override suspend fun send(text: String) = input.withLock {
        val session = terminal()?.takeIf { it.isReady } ?: throw DesktopRpcFailure("desktop_disconnected")
        if (TerminalPrompt.parse(screen()) != null) throw DesktopRpcFailure("agent_busy")
        // Bracketed paste keeps a multi-line message as one input; Enter then submits it.
        if (!session.paste(text)) throw DesktopRpcFailure("input_in_progress")
        delay(80)
        if (!session.sendText("\r")) throw DesktopRpcFailure("delivery_unknown")
        current.value = current.value.copy(canSend = false)
    }

    override suspend fun choose(prompt: ConversationPrompt, index: Int) = input.withLock {
        val session = terminal()?.takeIf { it.isReady } ?: throw DesktopRpcFailure("desktop_disconnected")
        val shown = TerminalPrompt.parse(screen())
        if (shown?.id != prompt.id || index !in shown.options.indices) throw DesktopRpcFailure("prompt_changed")
        val step = TerminalShortcut.parse(if (index > shown.selected) "↓" else "↑")!!
        repeat(kotlin.math.abs(index - shown.selected)) {
            if (!session.key(step.code, step.modifiers)) throw DesktopRpcFailure("delivery_unknown")
            delay(30)
        }
        val enter = TerminalShortcut.parse("Enter")!!
        if (!session.key(enter.code, enter.modifiers)) throw DesktopRpcFailure("delivery_unknown")
        current.value = current.value.copy(prompt = null)
    }

    override suspend fun key(key: String) = input.withLock {
        val session = terminal()?.takeIf { it.isReady } ?: throw DesktopRpcFailure("desktop_disconnected")
        val shortcut = TerminalShortcut.parse(key) ?: throw DesktopRpcFailure("conversation_unavailable")
        if (!session.key(shortcut.code, shortcut.modifiers, text = shortcut.text, unshifted = shortcut.unshifted)) {
            throw DesktopRpcFailure("delivery_unknown")
        }
    }

    private fun offset(id: String): Long = id.substringBefore(':').toLong()
}
