package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import io.github.kuddev.pebrel.terminal.TerminalShortcut
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Ordered key definitions only; this format never stores shell commands or terminal input. */
class ShortcutPreferences(context: Context) {
    private val stored = context.getSharedPreferences("terminal_shortcuts", Context.MODE_PRIVATE)
    private val current = MutableStateFlow(stored.getString("custom_v1", "").orEmpty().take(4096)
        .lineSequence().mapNotNull(TerminalShortcut::parse).map { it.label }.distinct().take(MAX_KEYS).toList())
    val state = current.asStateFlow()

    fun save(lines: List<String>): Boolean {
        if (lines.size > MAX_KEYS) return false
        val parsed = lines.map { TerminalShortcut.parse(it)?.label ?: return false }
        if (parsed.distinct().size != parsed.size) return false
        current.value = parsed
        stored.edit().putString("custom_v1", parsed.joinToString("\n")).apply()
        return true
    }

    companion object { const val MAX_KEYS = 24 }
}
