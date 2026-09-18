package io.github.kuddev.pebrel.mobile.connection

import android.view.KeyEvent
import io.github.kuddev.pebrel.terminal.TerminalInputTarget
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import org.json.JSONObject
import java.io.Closeable

/** One ordered, bounded writer per visible pane. Never replay after an uncertain
 * response or reconnect; authorization and the original client are rechecked.
 */
class DesktopTerminalInput(
    private val request: suspend (String, JSONObject) -> Unit,
    private val active: () -> Boolean,
    private val onAccepted: () -> Unit,
    private val onRejected: (Boolean) -> Unit,
) : TerminalInputTarget, Closeable {
    private data class Batch(val commands: List<Command>, val result: CompletableDeferred<Boolean>)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val queue = Channel<Batch>(64, onUndeliveredElement = { it.result.complete(false) })
    private var closed = false

    init {
        scope.launch {
            for (batch in queue) {
                try {
                    for (command in batch.commands) {
                        check(active())
                        request(command.method, command.params)
                    }
                    batch.result.complete(true)
                    onAccepted()
                } catch (cancelled: CancellationException) {
                    batch.result.complete(false)
                    throw cancelled
                } catch (_: Exception) {
                    batch.result.complete(false)
                    onRejected(true)
                    close()
                }
            }
        }
    }

    private fun enqueue(commands: List<Command>?): CompletableDeferred<Boolean>? {
        if (closed || !active() || commands == null) { onRejected(false); return null }
        val result = CompletableDeferred<Boolean>()
        if (queue.trySend(Batch(commands, result)).isFailure) { onRejected(false); return null }
        return result
    }

    override fun text(text: String): Boolean = enqueue(encodeText(text)) != null
    override fun paste(text: String): Boolean {
        // The old Runtime bridge has no bracketed-paste capability. Never turn
        // clipboard line breaks into implicit execution. Use the draft instead.
        if (text.any { it.isISOControl() }) { onRejected(false); return false }
        return text(text)
    }
    override fun key(code: Int, modifiers: Int, action: Int, text: String, unshifted: Int): Boolean {
        if (action == 0) return !closed && active() // Runtime owns key encoding, not key-up reports.
        return enqueue(encodeKey(code, modifiers, text)) != null
    }
    suspend fun submit(text: String): Boolean {
        val commands = encodeText(text)?.plus(keyCommand("enter"))
        return enqueue(commands)?.await() == true
    }
    override fun close() {
        closed = true
        queue.cancel()
        scope.cancel()
    }

    internal data class Command(val method: String, val params: JSONObject)
    companion object {
        internal fun encodeText(text: String): List<Command>? {
            if (text.toByteArray().size > 8192 || text.any { it.isISOControl() && it !in "\r\n\t" }) return null
            val commands = mutableListOf<Command>()
            val plain = StringBuilder()
            fun flush() {
                if (plain.isNotEmpty()) {
                    commands += Command("pane.prompt", JSONObject().put("text", plain.toString()).put("submit", false))
                    plain.clear()
                }
            }
            for (char in text.replace("\r\n", "\n")) {
                if (char in "\r\n\t") {
                    flush()
                    commands += keyCommand(if (char == '\t') "tab" else "enter")
                } else plain.append(char)
                if (commands.size > 128) return null
            }
            flush()
            return commands.takeIf { it.size <= 128 }
        }
        internal fun encodeKey(code: Int, modifiers: Int, text: String): List<Command>? {
            if (modifiers and 8 != 0) return null // No silent Meta-to-plain-text downgrade.
            val name = when (code) {
                KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "enter"
                KeyEvent.KEYCODE_ESCAPE -> "escape"
                KeyEvent.KEYCODE_TAB -> "tab"
                KeyEvent.KEYCODE_DEL -> "backspace"
                KeyEvent.KEYCODE_FORWARD_DEL -> "delete"
                KeyEvent.KEYCODE_DPAD_LEFT -> "left"
                KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
                KeyEvent.KEYCODE_DPAD_UP -> "up"
                KeyEvent.KEYCODE_DPAD_DOWN -> "down"
                KeyEvent.KEYCODE_MOVE_HOME -> "home"
                KeyEvent.KEYCODE_MOVE_END -> "end"
                KeyEvent.KEYCODE_INSERT -> "insert"
                KeyEvent.KEYCODE_PAGE_UP -> "page_up"
                KeyEvent.KEYCODE_PAGE_DOWN -> "page_down"
                in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> "f${code - KeyEvent.KEYCODE_F1 + 1}"
                in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z ->
                    if (modifiers and 2 != 0) ('a' + code - KeyEvent.KEYCODE_A).toString() else null
                else -> null
            }
            if (name != null) return listOf(keyCommand(name, modifiers))
            return if (text.isNotEmpty() && modifiers and 6 == 0) encodeText(text) else null
        }
        private fun keyCommand(name: String, modifiers: Int = 0) = Command("pane.send_key", JSONObject()
            .put("key", name).put("modifiers", JSONObject().put("shift", modifiers and 1 != 0)
                .put("control", modifiers and 2 != 0).put("alt", modifiers and 4 != 0)))
    }
}
