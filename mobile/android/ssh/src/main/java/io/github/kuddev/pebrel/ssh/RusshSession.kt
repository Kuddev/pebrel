package io.github.kuddev.pebrel.ssh

import java.io.Closeable
import java.io.InputStream
import java.io.OutputStream
import java.nio.CharBuffer
import java.util.concurrent.atomic.AtomicLong

/** Called on app-owned IO workers. Closing is safe during any native operation. */
class RusshSession private constructor(id: Long) : Closeable {
    private val nativeId = AtomicLong(id)
    private fun handle(): Long = nativeId.get().takeIf { it != 0L } ?: throw NativeSshException("CLOSED")

    fun connect(progress: (String) -> Unit, verify: (String) -> Boolean) {
        while (true) {
            val event = NativeSsh.nextEvent(handle())
            when {
                event == "connected" -> return
                event.startsWith("stage:") -> progress(event.removePrefix("stage:"))
                event.startsWith("verify:") -> {
                    progress("VERIFYING")
                    NativeSsh.answerHostKey(handle(), verify(event.removePrefix("verify:")))
                }
                event.startsWith("error:") -> throw NativeSshException(event.removePrefix("error:"))
                else -> throw NativeSshException("INTERNAL")
            }
        }
    }

    fun openShell(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) =
        NativeSsh.openShell(handle(), columns, rows, columns * cellWidth, rows * cellHeight)
    fun openExec(command: String) = NativeSsh.openExec(handle(), command)
    fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) =
        NativeSsh.resize(handle(), columns, rows, columns * cellWidth, rows * cellHeight)
    fun awaitExit(): Int = NativeSsh.awaitExit(handle())

    fun input(stderr: Boolean = false): InputStream = object : InputStream() {
        override fun read(): Int {
            val single = ByteArray(1)
            return if (read(single) < 0) -1 else single[0].toInt() and 255
        }
        @Synchronized override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
            requireRange(bytes, offset, length)
            if (length == 0) return 0
            return NativeSsh.read(handle(), bytes, offset, minOf(length, 16 * 1024), stderr)
        }
        override fun close() = this@RusshSession.close()
    }

    val output: OutputStream = object : OutputStream() {
        override fun write(value: Int) = write(byteArrayOf(value.toByte()))
        @Synchronized override fun write(bytes: ByteArray, offset: Int, length: Int) {
            requireRange(bytes, offset, length)
            var sent = 0
            while (sent < length) {
                val count = minOf(length - sent, 8192)
                NativeSsh.write(handle(), bytes, offset + sent, count)
                sent += count
            }
        }
        override fun close() = this@RusshSession.close()
    }

    override fun close() {
        val id = nativeId.getAndSet(0)
        if (id != 0L) NativeSsh.close(id)
    }

    companion object {
        fun create(host: String, port: Int, user: String, password: CharArray, fingerprint: String): RusshSession {
            val encoded = Charsets.UTF_8.encode(CharBuffer.wrap(password))
            val bytes = ByteArray(encoded.remaining())
            try {
                encoded.get(bytes)
                return RusshSession(NativeSsh.create(host, port, user, bytes, fingerprint))
            } finally {
                bytes.fill(0)
                if (encoded.hasArray()) encoded.array().fill(0)
                password.fill('\u0000')
            }
        }

        private fun requireRange(bytes: ByteArray, offset: Int, length: Int) {
            if (offset < 0 || length < 0 || offset > bytes.size - length) throw IndexOutOfBoundsException()
        }
    }
}
