package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Test

class FrameBoundsTest {
    @Test fun completeFrameLeavesNextFrameUnread() {
        val stream = "abc\nnext\n".byteInputStream()
        assertEquals("abc\n", readBoundedFrame(stream, 4)!!.toString(Charsets.UTF_8))
        assertEquals("next\n", readBoundedFrame(stream, 5)!!.toString(Charsets.UTF_8))
        assertNull(readBoundedFrame(stream, 5))
    }
    @Test fun oversizedAndTruncatedFramesAreRejected() {
        assertThrows(java.io.IOException::class.java) { readBoundedFrame("abc\n".byteInputStream(), 3) }
        assertThrows(java.io.EOFException::class.java) { readBoundedFrame("abc".byteInputStream(), 8) }
    }
}
