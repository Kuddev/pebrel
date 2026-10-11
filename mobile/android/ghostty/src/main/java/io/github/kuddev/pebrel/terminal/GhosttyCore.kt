package io.github.kuddev.pebrel.terminal

import java.io.Closeable

/** A row crosses JNI once; cells use six integers, never one Java object per cell. */
class TerminalRow(val text: String, val cells: IntArray)

data class TerminalHistory(val first: Long, val oldest: Long, val end: Long, val liveStart: Long,
                           val liveRows: Int, val applicationScroll: Boolean)

class TerminalFrame(val rows: Array<TerminalRow?>, val meta: IntArray, val wrapped: BooleanArray? = null,
                    val history: TerminalHistory? = null,
                    internal val sourceOffsets: IntArray? = null) {
    val columns get() = meta[0]
    val cursorX get() = meta[2]
    val cursorY get() = meta[3]
    val cursorVisible get() = meta[4] != 0
    val background get() = meta[5]
    val cursorColor get() = meta[6]
    val cursorStyle get() = meta[7]
    val scrollTotal get() = meta.getOrElse(8) { rows.size }
    val scrollOffset get() = meta.getOrElse(9) { 0 }
    fun text() = rows.joinToString("\n") { it?.text?.trimEnd().orEmpty() }.trimEnd()
}

/** Synchronization is a lifetime boundary; production calls run on a serial worker. */
class GhosttyCore(columns: Int = 80, rows: Int = 24, scrollbackLines: Int = 1000) : Closeable {
    private var handle: Long
    private var rowCache: Array<TerminalRow?>
    init {
        require(columns in 2..400 && rows in 2..200)
        require(scrollbackLines in 0..5000)
        handle = NativeBridge.create(columns, rows, scrollbackLines)
        check(handle != 0L)
        rowCache = arrayOfNulls(rows)
    }
    private fun pointer(): Long { check(handle != 0L) { "Terminal is closed" }; return handle }
    @Synchronized fun feed(bytes: ByteArray, count: Int = bytes.size): ByteArray {
        require(count in 0..bytes.size && count <= 65536)
        return NativeBridge.feed(pointer(), bytes, count)
    }
    @Synchronized fun takeTitle(): String? = NativeBridge.title(pointer())?.toString(Charsets.UTF_8)
    @Synchronized fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        require(columns in 2..400 && rows in 2..200 && cellWidth in 1..256 && cellHeight in 1..256)
        NativeBridge.resize(pointer(), columns, rows, cellWidth, cellHeight)
        if (rowCache.size != rows) rowCache = arrayOfNulls(rows)
    }
    @Synchronized fun snapshot(): TerminalFrame {
        val meta = IntArray(10)
        NativeBridge.render(pointer(), rowCache, meta)
        // Unchanged immutable rows are shared across frames; changed rows are replaced by JNI.
        return TerminalFrame(rowCache.copyOf(), meta)
    }
    @Synchronized fun scroll(lines: Int) = NativeBridge.scroll(pointer(), lines)
    @Synchronized fun scrollTo(offset: Int) = NativeBridge.scrollTo(pointer(), offset.coerceAtLeast(0))
    @Synchronized fun colors(colors: IntArray) {
        require(colors.size == 19)
        NativeBridge.colors(pointer(), colors)
    }
    @Synchronized fun key(code: Int, mods: Int, action: Int, text: String = "", unshifted: Int = 0): ByteArray =
        NativeBridge.key(pointer(), code, mods, action, text.toByteArray(), unshifted)
    @Synchronized fun paste(text: String): ByteArray {
        // Strip terminators that could escape bracketed paste and execute following text.
        val safe = text.replace("\u001b", "").replace("\u0000", "").replace("\r\n", "\n")
        return (if (NativeBridge.bracketedPaste(pointer())) "\u001b[200~$safe\u001b[201~" else safe).toByteArray()
    }
    @Synchronized override fun close() {
        if (handle != 0L) { NativeBridge.destroy(handle); handle = 0; rowCache = emptyArray() }
    }
}

internal object NativeBridge {
    init { System.loadLibrary("pebrel_ghostty") }
    external fun create(columns: Int, rows: Int, scrollbackLines: Int): Long
    external fun destroy(handle: Long)
    external fun feed(handle: Long, input: ByteArray, count: Int): ByteArray
    external fun title(handle: Long): ByteArray?
    external fun resize(handle: Long, columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    external fun render(handle: Long, rows: Array<TerminalRow?>, metadata: IntArray)
    external fun scroll(handle: Long, lines: Int)
    external fun scrollTo(handle: Long, offset: Int)
    external fun colors(handle: Long, colors: IntArray)
    external fun key(handle: Long, keyCode: Int, mods: Int, action: Int, text: ByteArray, unshifted: Int): ByteArray
    external fun bracketedPaste(handle: Long): Boolean
    external fun ptyOpen(directory: String, startup: String, columns: Int, rows: Int): IntArray
    external fun ptyResize(fd: Int, columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    external fun ptyStop(pid: Int)
    external fun ptyWait(pid: Int): Int
}
