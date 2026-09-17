package io.github.kuddev.pebrel.ssh

import java.io.Closeable
import java.util.Base64

/** No JVM crypto fallback: both endpoints use the versioned Rust protocol. */
class SecureMobileLink(hostPublicKey: String, credential: String, context: String) : Closeable {
    private var handle: Long
    init {
        val host = Base64.getUrlDecoder().decode(hostPublicKey)
        val secret = Base64.getUrlDecoder().decode(credential)
        try {
            require(host.size == 32 && secret.size == 32 && context.toByteArray().size <= 512)
            handle = NativeLink.create(host, secret, context.toByteArray())
        } finally { secret.fill(0) }
    }
    @Synchronized fun hello(): ByteArray = NativeLink.hello(active())
    @Synchronized fun finish(response: ByteArray) { check(NativeLink.finish(active(), response)) }
    @Synchronized fun seal(plaintext: ByteArray): Array<ByteArray> = NativeLink.seal(active(), plaintext)
    @Synchronized fun open(ciphertext: ByteArray): ByteArray? = NativeLink.open(active(), ciphertext)
    private fun active(): Long { check(handle != 0L); return handle }
    @Synchronized override fun close() {
        if (handle == 0L) return
        val previous = handle
        handle = 0
        NativeLink.close(previous)
    }
}

internal object NativeLink {
    init { System.loadLibrary("pebrel_ssh") }
    external fun create(host: ByteArray, secret: ByteArray, context: ByteArray): Long
    external fun hello(id: Long): ByteArray
    external fun finish(id: Long, response: ByteArray): Boolean
    external fun seal(id: Long, plaintext: ByteArray): Array<ByteArray>
    external fun open(id: Long, ciphertext: ByteArray): ByteArray?
    external fun close(id: Long)
}
