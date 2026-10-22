package io.github.kuddev.pebrel.terminal

/** Optional remote history owner. Called on the ordered IO queue, never on the UI/parser thread. */
interface TerminalHistoryTransport {
    /** True consumes the gesture; false leaves it to the terminal's normal wheel encoder. */
    fun scrollHistory(lines: Int): Boolean
    fun returnToLiveInput()
}
