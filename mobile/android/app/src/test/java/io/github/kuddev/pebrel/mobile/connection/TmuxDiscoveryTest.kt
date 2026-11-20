package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.*
import java.nio.file.Files
import java.util.concurrent.TimeUnit

/** Uses an isolated real remote multiplexer server when installed; no Android native library is needed. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = android.app.Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class TmuxDiscoveryTest {
    @Test fun metadataKeepsItsSeparatorsAndUnicodeWithoutAnSshLocale() {
        val available = runCatching {
            ProcessBuilder("tmux", "-V").start().let { it.waitFor(5, TimeUnit.SECONDS) && it.exitValue() == 0 }
        }.getOrDefault(false)
        assumeTrue("real remote multiplexer is not installed on this test host", available)
        val directory = Files.createTempDirectory("pebrel-session-").toFile()
        fun command(vararg args: String): Pair<Int, String> {
            val builder = ProcessBuilder(*args).redirectErrorStream(true)
            builder.environment().apply {
                remove("TMUX"); remove("LANG"); remove("LC_ALL"); remove("LC_CTYPE")
                put("TMUX_TMPDIR", directory.absolutePath)
            }
            val process = builder.start()
            if (!process.waitFor(5, TimeUnit.SECONDS)) {
                process.destroyForcibly()
                fail("isolated remote command timed out")
            }
            return process.exitValue() to process.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        }
        try {
            assertEquals(0, command("tmux", "-f", "/dev/null", "new-session", "-d", "-s", "fixture-测试",
                "-n", "代码", "/bin/sh").first)
            val (status, output) = command("sh", "-c", RemoteSessions.discoveryCommand)
            assertEquals(0, status)
            val session = RemoteSessions.parse(output).sessions.single { it.kind == "tmux" }
            assertEquals("fixture-测试", session.name)
            assertTrue(session.windows.single().label.endsWith("代码"))
            assertTrue(session.windows.single().pane.matches(Regex("%[0-9]+")))
        } finally {
            command("tmux", "kill-server")
            directory.deleteRecursively()
        }
    }
}
