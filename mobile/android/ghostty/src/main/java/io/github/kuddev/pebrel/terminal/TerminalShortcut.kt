package io.github.kuddev.pebrel.terminal

import android.view.KeyEvent
import java.util.Locale

/** Physical keys and modifier bits shared by SSH, local PTYs and desktop mirroring. */
data class TerminalShortcut(val label: String, val code: Int, val modifiers: Int, val text: String, val unshifted: Int) {
    fun send(target: TerminalInputTarget): Boolean = target.key(code, modifiers, text = text, unshifted = unshifted)

    companion object {
        val defaults = listOf("Ctrl+C", "Esc", "Tab", "Shift+Tab", "←", "→", "↑", "↓")
        private val modifiers = linkedMapOf("ctrl" to ("Ctrl" to 2), "alt" to ("Alt" to 4),
            "shift" to ("Shift" to 1), "meta" to ("Meta" to 8))
        private val named = mapOf(
            "esc" to ("Esc" to KeyEvent.KEYCODE_ESCAPE), "escape" to ("Esc" to KeyEvent.KEYCODE_ESCAPE),
            "tab" to ("Tab" to KeyEvent.KEYCODE_TAB), "enter" to ("Enter" to KeyEvent.KEYCODE_ENTER),
            "space" to ("Space" to KeyEvent.KEYCODE_SPACE), "backspace" to ("Backspace" to KeyEvent.KEYCODE_DEL),
            "delete" to ("Delete" to KeyEvent.KEYCODE_FORWARD_DEL), "insert" to ("Insert" to KeyEvent.KEYCODE_INSERT),
            "home" to ("Home" to KeyEvent.KEYCODE_MOVE_HOME), "end" to ("End" to KeyEvent.KEYCODE_MOVE_END),
            "pageup" to ("PageUp" to KeyEvent.KEYCODE_PAGE_UP), "pagedown" to ("PageDown" to KeyEvent.KEYCODE_PAGE_DOWN),
            "left" to ("←" to KeyEvent.KEYCODE_DPAD_LEFT), "←" to ("←" to KeyEvent.KEYCODE_DPAD_LEFT),
            "right" to ("→" to KeyEvent.KEYCODE_DPAD_RIGHT), "→" to ("→" to KeyEvent.KEYCODE_DPAD_RIGHT),
            "up" to ("↑" to KeyEvent.KEYCODE_DPAD_UP), "↑" to ("↑" to KeyEvent.KEYCODE_DPAD_UP),
            "down" to ("↓" to KeyEvent.KEYCODE_DPAD_DOWN), "↓" to ("↓" to KeyEvent.KEYCODE_DPAD_DOWN),
        )

        fun parse(value: String): TerminalShortcut? {
            if (value.length !in 1..64 || value.any(Char::isISOControl)) return null
            val parts = value.split('+').map { it.trim().lowercase(Locale.ROOT) }
            if (parts.any(String::isEmpty) || parts.size > 5) return null
            val selected = parts.dropLast(1)
            if (selected.distinct().size != selected.size || selected.any { it !in modifiers }) return null
            val bits = selected.fold(0) { result, item -> result or modifiers.getValue(item).second }
            val key = parts.last()
            val letter = key.singleOrNull()?.takeIf { it in 'a'..'z' }
            val digit = key.singleOrNull()?.takeIf { it in '0'..'9' }
            val function = key.removePrefix("f").toIntOrNull()?.takeIf { key.startsWith('f') && it in 1..12 }
            val (name, code) = when {
                letter != null -> letter.uppercase() to (KeyEvent.KEYCODE_A + (letter - 'a'))
                digit != null -> digit.toString() to (KeyEvent.KEYCODE_0 + (digit - '0'))
                function != null -> "F$function" to (KeyEvent.KEYCODE_F1 + function - 1)
                else -> named[key] ?: return null
            }
            val raw = letter ?: digit ?: if (key == "space") ' ' else null
            val text = when {
                raw == null -> ""
                letter != null && bits and 1 != 0 -> letter.uppercase()
                digit != null && bits and 1 != 0 -> ")!@#$%^&*("[digit - '0'].toString()
                else -> raw.toString()
            }
            val label = (modifiers.filterKeys { it in selected }.values.map { it.first } + name).joinToString("+")
            return TerminalShortcut(label, code, bits, text, raw?.code ?: 0)
        }
    }
}
