package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

/** Transport lifetime is separate from RPC identity and the desktop's task state. */
interface DesktopTransport : Closeable {
    suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit)
    fun send(frame: JSONObject)
    fun readyTimeoutFailure(): DesktopFailureKind = DesktopFailureKind.PEER_OFFLINE
}

class SshDesktopTransport(private val connection: SshConnection) : DesktopTransport {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val closed = AtomicBoolean()

    override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
        withContext(Dispatchers.IO) {
            connection.connect()
            check(!closed.get())
            connection.openExec(if (allowInput) "pebrel mobile-bridge --allow-input" else "pebrel mobile-bridge")
            check(!closed.get())
            scope.launch {
                var failure: Throwable? = null
                try {
                    BufferedInputStream(connection.input(), 8192).use { input ->
                        while (!closed.get()) {
                            val bytes = readBoundedFrame(input, 2 * 1024 * 1024) ?: break
                            receive(JSONObject(bytes.toString(Charsets.UTF_8)))
                        }
                    }
                } catch (error: Exception) { failure = error }
                finally { if (!closed.get()) disconnected(failure); close() }
            }
            scope.launch {
                runCatching {
                    val input = connection.input(stderr = true)
                    val scratch = ByteArray(4096)
                    while (!closed.get() && input.read(scratch) >= 0) { }
                }
            }
        }
    }
    override fun send(frame: JSONObject) {
        check(!closed.get())
        connection.output().apply { write((frame.toString() + "\n").toByteArray()); flush() }
    }
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        scope.cancel()
        connection.close()
    }
}
