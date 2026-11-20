package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.nio.file.Files
import java.util.concurrent.TimeUnit

class TmuxRecoveryCommandTest {
    @Test fun missingSessionPaneAndReusedIdentityFallBackToShellWithoutCreatingSessions() {
        assumeTrue(runCatching { ProcessBuilder("tmux", "-V").start().waitFor() == 0 }.getOrDefault(false))
        val directory = Files.createTempDirectory("pebrel-resume-").toFile()
        val socket = directory.resolve("named socket").absolutePath
        val shell = directory.resolve("shell").apply { writeText("#!/bin/sh\nprintf 'FALLBACK:%s\\n' \"${'$'}1\"\n"); setExecutable(true) }
        fun run(vararg args: String): Pair<Int, String> {
            val process = ProcessBuilder(*args).redirectErrorStream(true).apply {
                environment().remove("TMUX")
                if (args.first() == "sh") environment()["SHELL"] = shell.absolutePath
            }.start()
            check(process.waitFor(5, TimeUnit.SECONDS)) { process.destroyForcibly(); "command did not finish" }
            return process.exitValue() to process.inputStream.bufferedReader().readText()
        }
        try {
            assertEquals(0, run("tmux", "-S", socket, "-f", "/dev/null", "new-session", "-d", "-s", "original", "/bin/sh").first)
            val fields = run("tmux", "-S", socket, "display-message", "-p", "-t", "original",
                "#{session_id}\t#{session_created}:#{pid}\t#{window_id}\t#{pane_id}").second.trim().split('\t')
            val target = RemoteAttachment(RemoteSession("tmux", fields[0], "original", fields[1],
                endpoint = TmuxEndpoint("tmux", socket)), RemoteWindow(fields[2], "original", pane = fields[3]))
            fun fallback(attachment: RemoteAttachment) {
                val (status, text) = run("sh", "-c", RemoteSessions.attachCommand(attachment))
                assertEquals(text, 0, status)
                assertTrue(text, text.endsWith("FALLBACK:-l\n"))
            }
            fallback(target.copy(session = target.session.copy(identity = "0:0")))
            fallback(target.copy(window = target.window!!.copy(pane = "%999999")))
            // The guard succeeds, but attachment has no PTY here and fails. Failure after
            // validation (including a concurrently removed target) must still open a shell.
            fallback(target)
            assertEquals(0, run("tmux", "-S", socket, "has-session", "-t", "original").first)
            run("tmux", "-S", socket, "kill-server")
            fallback(target)
            assertNotEquals(0, run("tmux", "-S", socket, "has-session", "-t", "original").first)
        } finally {
            run("tmux", "-S", socket, "kill-server")
            directory.deleteRecursively()
        }
    }
}
