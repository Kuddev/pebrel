package io.github.kuddev.pebrel.mobile.connection

import io.github.kuddev.pebrel.ssh.NativeSshException
import io.github.kuddev.pebrel.ssh.RusshSession
import io.github.kuddev.pebrel.terminal.SessionTransport
import java.io.Closeable
import java.io.InputStream
import java.io.OutputStream

data class HostProfile(
    val id: String, val name: String, val address: String, val port: Int = 22,
    val user: String, val fingerprint: String = "",
    val icon: String = "term", val group: String = "development",
)

/** Kotlin owns UI trust and lifetime; russh owns SSH negotiation and encrypted IO. */
class SshConnection(
    private val host: HostProfile,
    private val password: CharArray,
    private val verify: (HostProfile, String) -> Boolean,
    private val progress: (SshStage) -> Unit = {},
) : Closeable {
    private val guard = Any()
    @Volatile private var opened: RusshSession? = null
    private var closed = false
    private fun session(): RusshSession = opened ?: throw NativeSshException("CLOSED")

    fun connect() {
        try {
            val transport = synchronized(guard) {
                if (closed) throw NativeSshException("CLOSED")
                check(opened == null)
                RusshSession.create(host.address, host.port, host.user, password, host.fingerprint).also { opened = it }
            }
            transport.connect({ progress(SshStage.valueOf(it)) }, { fingerprint -> verify(host, fingerprint) })
        } catch (error: Exception) {
            close()
            throw SshFailure(classifySshFailure(error), error)
        } finally { password.fill('\u0000') }
    }

    fun openShell(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        progress(SshStage.OPENING_SHELL)
        session().openShell(columns, rows, cellWidth, cellHeight)
    }
    fun openExec(command: String) {
        progress(SshStage.OPENING_SHELL)
        session().openExec(command)
    }
    fun input(stderr: Boolean = false): InputStream = session().input(stderr)
    fun output(): OutputStream = session().output
    fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = session().resize(columns, rows, cellWidth, cellHeight)
    fun awaitExit(): Int = session().awaitExit()

    override fun close() {
        val transport = synchronized(guard) {
            closed = true
            password.fill('\u0000')
            opened.also { opened = null }
        }
        transport?.close()
    }
}

/** Ghostty owns VT, IME and selection; the independent russh module supplies bytes. */
class SshTerminalTransport(private val connection: SshConnection) : SessionTransport {
    override fun open(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) {
        connection.connect()
        connection.openShell(columns, rows, cellWidth, cellHeight)
    }
    override fun input(): InputStream = connection.input()
    override fun output(): OutputStream = connection.output()
    override fun resize(columns: Int, rows: Int, cellWidth: Int, cellHeight: Int) = connection.resize(columns, rows, cellWidth, cellHeight)
    override fun awaitExit(): Int = connection.awaitExit()
    override fun close() = connection.close()
}
