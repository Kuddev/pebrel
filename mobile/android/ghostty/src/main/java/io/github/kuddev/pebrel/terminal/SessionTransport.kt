package io.github.kuddev.pebrel.terminal

import android.os.ParcelFileDescriptor
import java.io.Closeable
import java.io.InputStream
import java.io.OutputStream

/** The transport owns only byte I/O and PTY geometry. Every call runs off the UI thread. */
interface SessionTransport : Closeable {
    fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    fun input(): InputStream
    fun output(): OutputStream
    fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    fun awaitExit(): Int
}

class LocalPtyTransport(private val directory: String) : SessionTransport {
    private var master: ParcelFileDescriptor? = null
    private var reader: InputStream? = null
    private var writer: OutputStream? = null
    private var child = 0
    private var closed = false
    @Synchronized override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        check(!closed)
        val process = NativeBridge.ptyOpen(directory, columns, rows)
        child = process[1]
        val descriptor = ParcelFileDescriptor.adoptFd(process[0])
        master = descriptor
        reader = ParcelFileDescriptor.AutoCloseInputStream(ParcelFileDescriptor.dup(descriptor.fileDescriptor))
        writer = ParcelFileDescriptor.AutoCloseOutputStream(ParcelFileDescriptor.dup(descriptor.fileDescriptor))
        resize(columns, rows, cellWidth, cellHeight)
    }
    override fun input(): InputStream = checkNotNull(reader)
    override fun output(): OutputStream = checkNotNull(writer)
    @Synchronized override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        if (!closed) master?.let { NativeBridge.ptyResize(it.fd, columns, rows, cellWidth, cellHeight) }
    }
    override fun awaitExit(): Int {
        while (true) {
            val status = synchronized(this) {
                if (child <= 0) return -1
                NativeBridge.ptyWait(child).also { if (it != Int.MIN_VALUE) child = 0 }
            }
            if (status != Int.MIN_VALUE) return status
            Thread.sleep(20)
        }
    }
    @Synchronized override fun close() {
        if (closed) return
        closed = true
        if (child > 0) NativeBridge.ptyStop(child)
        runCatching { reader?.close() }
        runCatching { writer?.close() }
        runCatching { master?.close() }
        master = null
    }
}
