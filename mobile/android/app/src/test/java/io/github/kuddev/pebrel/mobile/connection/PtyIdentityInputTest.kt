package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayInputStream

class PtyIdentityInputTest {
    @Test fun stripsOnlyItsOwnMarkerAndPreservesBannerAndTerminalOutput() {
        var tty: String? = null
        val body = "banner\r\n\u001ePEBREL_TTY_nonce=/dev/pts/7\u001f\u001b[31mλ终端\r\n"
        val input = PtyIdentityInput(ByteArrayInputStream(body.toByteArray()), "nonce") { tty = it }
        assertEquals("banner\r\n\u001b[31mλ终端\r\n", input.readBytes().toString(Charsets.UTF_8))
        assertEquals("/dev/pts/7", tty)
    }

    @Test fun mismatchedTruncatedAndInvalidMarkersArePreservedWithoutChoosingATarget() {
        for (body in listOf("\u001ePEBREL_TTY_other=/dev/pts/3\u001f", "\u001ePEBREL_TTY_nonce=",
            "\u001ePEBREL_TTY_nonce=$(bad)\u001f", "a".repeat(9000))) {
            val input = PtyIdentityInput(ByteArrayInputStream(body.toByteArray()), "nonce") { fail("unexpected target") }
            assertArrayEquals(body.toByteArray(), input.readBytes())
        }
    }
}
