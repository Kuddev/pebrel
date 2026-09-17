package io.github.kuddev.pebrel.mobile

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.terminal.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.Executors

/** The optimized APK talks to a real OpenSSH fixture on the emulator host. */
@RunWith(AndroidJUnit4::class)
class SshIntegrationTest {
    private val host = HostProfile("ci-ssh", "CI OpenSSH", "10.0.2.2", 2222, "pebreltest")
    private val expectedFingerprint get() = requireNotNull(
        InstrumentationRegistry.getArguments().getString("sshFingerprint"))

    @Test fun realPasswordShellStreamsOutputThroughGhostty() {
        val ready = CountDownLatch(1)
        val finished = CountDownLatch(1)
        val stages = Collections.synchronizedList(mutableListOf<SshStage>())
        val secret = "pebrel-test-only".toCharArray()
        val connection = SshConnection(host, secret, { _, fingerprint -> fingerprint == expectedFingerprint }, { stages.add(it) })
        val terminal = TerminalSession(SshTerminalTransport(connection), object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
            override fun onSessionFinished(session: TerminalSession) { finished.countDown(); ready.countDown() }
        })
        try {
            terminal.setVisible(true)
            terminal.start()
            assertTrue("SSH did not settle", ready.await(25, TimeUnit.SECONDS))
            terminal.failureCause?.let { throw AssertionError("Real OpenSSH connection failed", it) }
            assertNull(terminal.failure)
            assertEquals(listOf(SshStage.NETWORK, SshStage.VERIFYING, SshStage.AUTHENTICATING, SshStage.OPENING_SHELL), stages.toList())
            assertTrue(secret.all { it == '\u0000' })
            assertTrue(terminal.sendText("printf '\\123\\123\\110_GHOSTTY_OK\\n'\r"))
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
            while (terminal.frame?.rows?.any { it?.text?.contains("SSH_GHOSTTY_OK") == true } != true && System.nanoTime() < deadline) Thread.sleep(50)
            assertTrue(terminal.frame?.rows?.any { it?.text?.contains("SSH_GHOSTTY_OK") == true } == true)
            assertTrue(terminal.sendText("exit\r"))
            assertTrue(finished.await(10, TimeUnit.SECONDS))
            assertFalse(terminal.sendText("must-not-send"))
        } finally { terminal.finishIfRunning() }
    }

    @Test fun wrongPasswordReportsAuthenticationAndWipesSecret() {
        val secret = "wrong-test-password".toCharArray()
        SshConnection(host.copy(fingerprint = expectedFingerprint), secret, { _, _ -> error("Already trusted") }).use { connection ->
            val error = runCatching { connection.connect() }.exceptionOrNull()
            assertTrue(error is SshFailure)
            assertEquals(error?.stackTraceToString(), SshFailureKind.AUTH, (error as SshFailure).kind)
            assertTrue(secret.all { it == '\u0000' })
        }
    }

    @Test fun changedHostKeyNeverReachesAuthentication() {
        val stages = Collections.synchronizedList(mutableListOf<SshStage>())
        SshConnection(host.copy(fingerprint = "SHA256:wrong-fixture-key"), "pebrel-test-only".toCharArray(),
            { _, _ -> error("Changed identity must not prompt as new") }, { stages.add(it) }).use { connection ->
            val error = runCatching { connection.connect() }.exceptionOrNull()
            assertTrue(error is SshFailure)
            assertEquals(error?.stackTraceToString(), SshFailureKind.HOST_KEY_CHANGED, (error as SshFailure).kind)
            assertFalse(stages.contains(SshStage.AUTHENTICATING))
        }
    }

    @Test fun declinedTrustNeverAuthenticates() {
        val stages = Collections.synchronizedList(mutableListOf<SshStage>())
        SshConnection(host, "pebrel-test-only".toCharArray(), { _, _ -> false }, { stages.add(it) }).use { connection ->
            val error = runCatching { connection.connect() }.exceptionOrNull()
            assertTrue(error is SshFailure)
            assertEquals(error?.stackTraceToString(), SshFailureKind.TRUST_REJECTED, (error as SshFailure).kind)
            assertFalse(stages.contains(SshStage.AUTHENTICATING))
        }
    }

    @Test fun execKeepsStderrOutOfRpcStdoutAndPreservesExitCode() {
        SshConnection(host.copy(fingerprint = expectedFingerprint), "pebrel-test-only".toCharArray(),
            { _, _ -> error("Already trusted") }).use { connection ->
            connection.connect()
            connection.openExec("printf 'rpc-output'; printf 'diagnostic' >&2; exit 7")
            assertEquals("rpc-output", connection.input().readBytes().toString(Charsets.UTF_8))
            assertEquals("diagnostic", connection.input(stderr = true).readBytes().toString(Charsets.UTF_8))
            assertEquals(7, connection.awaitExit())
        }
    }

    @Test fun closingPendingNativeReadUnblocksAndNextConnectionStillWorks() {
        val executor = Executors.newSingleThreadExecutor()
        try {
            repeat(2) {
                SshConnection(host.copy(fingerprint = expectedFingerprint), "pebrel-test-only".toCharArray(),
                    { _, _ -> error("Already trusted") }).use { connection ->
                    connection.connect()
                    connection.openExec("printf ready; sleep 60")
                    val input = connection.input()
                    val ready = ByteArray(5)
                    var read = 0
                    while (read < ready.size) {
                        val count = input.read(ready, read, ready.size - read)
                        assertTrue(count > 0)
                        read += count
                    }
                    assertEquals("ready", ready.toString(Charsets.UTF_8))
                    val reading = CountDownLatch(1)
                    val result = executor.submit<Boolean> {
                        reading.countDown()
                        runCatching { input.read() }.isFailure
                    }
                    assertTrue(reading.await(2, TimeUnit.SECONDS))
                    connection.close()
                    assertTrue(result.get(3, TimeUnit.SECONDS))
                    assertTrue(runCatching { connection.output().write(1) }.isFailure)
                }
            }
        } finally { executor.shutdownNow() }
    }
}
