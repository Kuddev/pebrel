package io.github.kuddev.pebrel.ssh

import java.io.IOException

/** Fixed native error codes; no server-provided text or credentials cross into UI. */
class NativeSshException @JvmOverloads constructor(val code: String, cause: Throwable? = null) : IOException(code, cause)

internal object NativeSsh {
    init { System.loadLibrary("pebrel_ssh") }
    external fun create(host: String, port: Int, user: String, password: ByteArray, fingerprint: String): Long
    external fun nextEvent(id: Long): String
    external fun answerHostKey(id: Long, accepted: Boolean)
    external fun openShell(id: Long, columns: Int, rows: Int, width: Int, height: Int)
    external fun openExec(id: Long, command: String)
    external fun read(id: Long, bytes: ByteArray, offset: Int, count: Int, stderr: Boolean): Int
    external fun write(id: Long, bytes: ByteArray, offset: Int, count: Int)
    external fun resize(id: Long, columns: Int, rows: Int, width: Int, height: Int)
    external fun awaitExit(id: Long): Int
    external fun close(id: Long)
}
