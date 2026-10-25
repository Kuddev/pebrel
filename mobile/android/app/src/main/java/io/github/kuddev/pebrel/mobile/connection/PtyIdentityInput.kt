package io.github.kuddev.pebrel.mobile.connection

import java.io.InputStream
import java.util.ArrayDeque

/** Removes only our nonce-scoped startup marker. Banners and all later output stay byte-exact. */
internal class PtyIdentityInput(source: InputStream, token: String, private val identified: (String) -> Unit) : InputStream() {
    private val source = source.buffered(16 * 1024)
    private val prefix = "\u001ePEBREL_TTY_$token=".toByteArray()
    private val candidate = ArrayList<Int>()
    private val output = ArrayDeque<Int>()
    private var scanned = 0
    private var done = false

    override fun read(): Int {
        while (output.isEmpty()) {
            if (done) return source.read()
            val byte = source.read()
            if (byte < 0) {
                output.addAll(candidate); candidate.clear(); done = true
                if (output.isEmpty()) return -1
                break
            }
            scanned++
            if (candidate.size < prefix.size) {
                if (byte == prefix[candidate.size].toInt()) candidate.add(byte)
                else {
                    output.addAll(candidate); candidate.clear()
                    if (byte == prefix[0].toInt()) candidate.add(byte) else output.add(byte)
                }
            } else if (byte == 0x1f) {
                val tty = candidate.drop(prefix.size).map(Int::toChar).joinToString("")
                if (tty.matches(Regex("/dev/[A-Za-z0-9/_-]{1,128}"))) identified(tty)
                else { output.addAll(candidate); output.add(byte) }
                candidate.clear(); done = true
            } else candidate.add(byte)
            if (scanned >= 8192 || candidate.size > prefix.size + 256) {
                output.addAll(candidate); candidate.clear(); done = true
            }
        }
        return output.removeFirst()
    }

    override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
        require(offset >= 0 && length >= 0 && offset <= bytes.size - length)
        if (length == 0) return 0
        if (done && output.isEmpty()) return source.read(bytes, offset, length)
        val first = read()
        if (first < 0) return -1
        bytes[offset] = first.toByte()
        var count = 1
        while (count < length && (output.isNotEmpty() || source.available() > 0)) {
            val next = read()
            if (next < 0) break
            bytes[offset + count++] = next.toByte()
        }
        return count
    }
    override fun close() = source.close()
}
