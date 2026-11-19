package io.github.kuddev.pebrel.mobile.connection

import android.view.KeyEvent
import io.github.kuddev.pebrel.terminal.TerminalInputTarget
import io.github.kuddev.pebrel.terminal.TerminalShortcut
import org.junit.Assert.*
import org.junit.Test

class TerminalShortcutTest {
    @Test fun ctrlShiftRetainsThePhysicalKeyModifiersAndUnshiftedCodepoint() {
        val key = checkNotNull(TerminalShortcut.parse(" shift + CTRL + x "))
        assertEquals("Ctrl+Shift+X", key.label)
        assertEquals(KeyEvent.KEYCODE_X, key.code)
        assertEquals(3, key.modifiers)
        assertEquals("X", key.text)
        assertEquals('x'.code, key.unshifted)
        var sent: List<Any>? = null
        val target = object : TerminalInputTarget {
            override fun text(text: String) = error("must dispatch a key, not shell text")
            override fun key(code: Int, modifiers: Int, action: Int, text: String, unshifted: Int): Boolean {
                sent = listOf(code, modifiers, action, text, unshifted); return true
            }
        }
        assertTrue(key.send(target))
        assertEquals(listOf(KeyEvent.KEYCODE_X, 3, 1, "X", 'x'.code), sent)
    }

    @Test fun namedFunctionAndNavigationKeysUseTheSameMappingAsDefaultButtons() {
        TerminalShortcut.defaults.forEach { assertNotNull(it, TerminalShortcut.parse(it)) }
        assertEquals(KeyEvent.KEYCODE_F12, TerminalShortcut.parse("Ctrl+Alt+F12")?.code)
        assertEquals(6, TerminalShortcut.parse("Ctrl+Alt+F12")?.modifiers)
        assertEquals("Shift+←", TerminalShortcut.parse("Shift+Left")?.label)
        assertEquals(KeyEvent.KEYCODE_MOVE_END, TerminalShortcut.parse("Meta+End")?.code)
        assertEquals("!", TerminalShortcut.parse("Shift+1")?.text)
        assertEquals(" ", TerminalShortcut.parse("Ctrl+Space")?.text)
    }

    @Test fun invalidDefinitionsNeverBecomeAnotherKeyOrRawCommands() {
        for (value in listOf("", "Ctrl++X", "Ctrl+Ctrl+X", "Ctrl+Shift", "Ctrl+Unknown", "F13",
            "Ctrl+X\nrm -rf", "echo test", "Ctrl+Shift+xxx", "Alt+Enter;pwd", "a".repeat(100))) {
            assertNull(value, TerminalShortcut.parse(value))
        }
    }
}
