package io.github.kuddev.pebrel.terminal

/** The same IME contract for a local PTY, SSH PTY and a mirrored desktop pane.
 * Acceptance means queued, not remotely executed. Owners must reject stale input.
 */
interface TerminalInputTarget {
    fun text(text: String): Boolean
    fun key(code: Int, modifiers: Int = 0, action: Int = 1, text: String = "", unshifted: Int = 0): Boolean
    fun paste(text: String): Boolean = text(text)
}
