package io.github.kuddev.pebrel.terminal

import android.os.ParcelFileDescriptor
import java.io.Closeable
import java.io.InputStream
import java.io.OutputStream
import java.io.File

/** The transport owns only byte I/O and PTY geometry. Every call runs off the UI thread. */
interface SessionTransport : Closeable {
    fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    fun input(): InputStream
    fun output(): OutputStream
    fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int)
    fun awaitExit(): Int
}

class LocalPtyTransport(private val directory: String, private val configDirectory: String) : SessionTransport {
    private var master: ParcelFileDescriptor? = null
    private var reader: InputStream? = null
    private var writer: OutputStream? = null
    private var child = 0
    private var closed = false
    @Synchronized override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        check(!closed)
        val process = NativeBridge.ptyOpen(directory, prepareShell(configDirectory), columns, rows)
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

    private fun prepareShell(directory: String): String {
        val root = File(directory)
        check(root.mkdirs() || root.isDirectory)
        val rc = File(root, "pebrel-shell.rc")
        val contents = """
            # 仅配置交互 shell；--color=auto 保证重定向和管道不夹带颜色转义。
            case ${'$'}- in *i*) alias ls='ls --color=auto' ;; esac
            # mksh expands PWD at each prompt; single quotes keep cd updates live.
            PS1='${'$'}{PWD} ${'$'} '
            if [ -r "${'$'}HOME/.mkshrc" ]; then . "${'$'}HOME/.mkshrc"; fi
        """.trimIndent() + "\n"
        if (!rc.isFile || rc.readText(Charsets.UTF_8) != contents) {
            val pending = File.createTempFile("shell-", ".rc", root)
            try {
                pending.writeText(contents, Charsets.UTF_8)
                // 不覆盖用户的 shell 文件；原子替换自有配置，避免同时开多个终端读到半份内容。
                android.system.Os.rename(pending.absolutePath, rc.absolutePath)
            } finally { pending.delete() }
        }
        return rc.absolutePath
    }
}
