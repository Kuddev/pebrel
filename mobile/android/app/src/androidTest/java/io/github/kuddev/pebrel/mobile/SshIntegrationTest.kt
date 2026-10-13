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
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.security.MessageDigest

/** The optimized APK talks to a real OpenSSH fixture on the emulator host. */
@RunWith(AndroidJUnit4::class)
class SshIntegrationTest {
    private val arguments get() = InstrumentationRegistry.getArguments()
    private val host = HostProfile("ci-ssh", "CI OpenSSH", arguments.getString("sshHost") ?: "10.0.2.2",
        arguments.getString("sshPort")?.toInt() ?: 2222, arguments.getString("sshUser") ?: "pebreltest")
    private val expectedFingerprint get() = requireNotNull(
        InstrumentationRegistry.getArguments().getString("sshFingerprint"))

    @Test fun privateKeysAuthenticateWithPassphrasesAndNeverFallBackToPassword() {
        val trusted = host.copy(fingerprint = expectedFingerprint)
        fun command(text: String): ByteArray = SshConnection(trusted, "pebrel-test-only".toCharArray(),
            { _, _ -> error("Already trusted") }).use { connection ->
            connection.connect(); connection.openExec(text)
            val output = connection.input().readBytes()
            assertEquals("fixture command failed", 0, connection.awaitExit())
            output
        }
        for ((kind, passphrase, accepted) in listOf(
            Triple("ed25519", "", true), Triple("ed25519", "pebrel-key-test-only", true),
            Triple("rsa", "", true), Triple("ed25519", "", false))) {
            val marker = "pebrel-key-${java.util.UUID.randomUUID()}"
            val directory = ".$marker"
            val format = if (kind == "rsa") "-b 2048 -m PEM" else ""
            val authorize = if (accepted) "cat '$directory/key.pub' >> .ssh/authorized_keys; chmod 600 .ssh/authorized_keys;" else ""
            val key = command("umask 077; mkdir '$directory'; mkdir -p .ssh; chmod 700 .ssh; " +
                "ssh-keygen -q -t $kind $format -N '$passphrase' -C '$marker' -f '$directory/key'; $authorize cat '$directory/key'")
            try {
                val profile = trusted.copy(keyUri = "content://fixture/$marker")
                val secret = (if (accepted) passphrase else "pebrel-test-only").toCharArray()
                SshConnection(profile, secret, { _, _ -> error("Already trusted") }, keySource = { key.copyOf() }).use { connection ->
                    val failure = runCatching { connection.connect() }.exceptionOrNull()
                    assertTrue(secret.all { it == '\u0000' })
                    if (accepted) {
                        assertNull(failure)
                        connection.openExec("printf key-authenticated; sleep 10")
                        val expected = "key-authenticated".toByteArray(Charsets.UTF_8)
                        val output = ByteArray(expected.size)
                        var offset = 0
                        val input = connection.input()
                        while (offset < output.size) {
                            val count = input.read(output, offset, output.size - offset)
                            assertTrue(count > 0)
                            offset += count
                        }
                        assertArrayEquals(expected, output)
                        assertEquals("file", connection.sftp(org.json.JSONObject().put("op", "stat").put("path", "$directory/key.pub")).getString("kind"))
                    } else {
                        assertEquals("a valid password must not replace a rejected selected key", SshFailureKind.AUTH, (failure as SshFailure).kind)
                    }
                }
                if (passphrase.isNotEmpty()) {
                    val wrong = "wrong-key-passphrase".toCharArray()
                    SshConnection(profile, wrong, { _, _ -> error("Already trusted") }, keySource = { key.copyOf() }).use { connection ->
                        val error = runCatching { connection.connect() }.exceptionOrNull()
                        assertEquals(SshFailureKind.KEY, (error as SshFailure).kind)
                        assertTrue(wrong.all { it == '\u0000' })
                    }
                }
            } finally {
                key.fill(0)
                command("if [ -f .ssh/authorized_keys ]; then sed -i '/ $marker\$/d' .ssh/authorized_keys; fi; rm -f '$directory/key' '$directory/key.pub'; rmdir '$directory'").fill(0)
            }
        }
    }

    @Test fun tmuxReattachesTheSameRemoteShellAndKeepsSftpAvailable() = runBlocking {
        persistentSession(SshSessionMode.TMUX)
    }

    @Test fun herdrReattachesTheSameRemoteShellAndKeepsSftpAvailable() = runBlocking {
        persistentSession(SshSessionMode.HERDR)
    }

    private suspend fun persistentSession(mode: SshSessionMode) {
        val name = "pebrel-qa-${System.nanoTime()}"
        val target = host.copy(fingerprint = expectedFingerprint, sessionMode = mode, sessionName = name)
        fun connection(profile: HostProfile = target) = SshConnection(profile, "pebrel-test-only".toCharArray(),
            { _, _ -> error("The fixture fingerprint must already match") })
        suspend fun opened(): Pair<TerminalSession, SshConnection> {
            val ready = CompletableDeferred<Unit>()
            val channel = connection()
            val terminal = TerminalSession(SshTerminalTransport(channel), object : TerminalCallbacks() {
                override fun onTransportReady(session: TerminalSession) { ready.complete(Unit) }
                override fun onSessionFinished(session: TerminalSession) { ready.complete(Unit) }
            })
            terminal.setVisible(true)
            terminal.start()
            try {
                withTimeout(25_000) { ready.await() }
                terminal.failureCause?.let { throw AssertionError("Persistent SSH failed", it) }
                try {
                    withTimeout(15_000) {
                        // 全屏 TUI 可以自行绘制光标；后面的真实命令/PID断言才判定可交互和重连。
                        while (terminal.frame?.text().isNullOrBlank()) delay(50)
                    }
                } catch (error: TimeoutCancellationException) {
                    throw AssertionError("${mode.id} initial frame: ${terminal.frame?.meta?.contentToString()}\n${terminal.frame?.text()?.take(2400)}", error)
                }
                return terminal to channel
            } catch (error: Throwable) { terminal.finishIfRunning(); throw error }
        }
        suspend fun output(terminal: TerminalSession, pattern: Regex): String = withTimeout(12_000) {
            while (true) {
                terminal.failureCause?.let { throw AssertionError("Persistent session stopped", it) }
                pattern.find(terminal.frame?.text().orEmpty())?.let { return@withTimeout it.value }
                delay(50)
            }
            @Suppress("UNREACHABLE_CODE") ""
        }
        try {
            val (first, ssh) = opened()
            val pid = try {
                // 标记首字符使用八进制，避免把命令回显误当成远端执行结果。
                assertTrue(first.sendText("PEBREL_QA_TOKEN=$name; export PEBREL_QA_TOKEN; printf '\\115UX_FIRST:%s:%s\\n' \"\$PEBREL_QA_TOKEN\" \"\$\$\"\r"))
                val marker = output(first, Regex("MUX_FIRST:$name:[0-9]+"))
                assertTrue(SftpClient(ssh::sftp).list(".").path.startsWith('/'))
                marker.substringAfterLast(':')
            } finally { first.finishIfRunning() }
            val (second, _) = opened()
            try {
                assertTrue(second.sendText("printf '\\115UX_SECOND:%s:%s\\n' \"\$PEBREL_QA_TOKEN\" \"\$\$\"\r"))
                assertEquals("MUX_SECOND:$name:$pid", output(second, Regex("MUX_SECOND:$name:[0-9]+")))
            } finally { second.finishIfRunning() }
        } finally {
            withContext(Dispatchers.IO + NonCancellable) {
                connection(host.copy(fingerprint = expectedFingerprint)).use { cleanup ->
                    cleanup.connect()
                    cleanup.openExec(if (mode == SshSessionMode.TMUX) "tmux kill-session -t '$name'" else "herdr session stop '$name'")
                    cleanup.input().readBytes()
                    cleanup.awaitExit()
                }
            }
        }
    }

    @Test fun sftpRoundTripPagingAndCancellationKeepTheRealShellUsable() = runBlocking {
        val ready = CountDownLatch(1)
        val connection = SshConnection(host.copy(fingerprint = expectedFingerprint), "pebrel-test-only".toCharArray(), { _, _ -> error("Already trusted") })
        val terminal = TerminalSession(SshTerminalTransport(connection), object : TerminalCallbacks() {
            override fun onTransportReady(session: TerminalSession) { ready.countDown() }
            override fun onSessionFinished(session: TerminalSession) { ready.countDown() }
        })
        val files = SftpClient(connection::sftp)
        try {
            terminal.setVisible(true); terminal.start()
            assertTrue(ready.await(25, TimeUnit.SECONDS))
            terminal.failureCause?.let { throw AssertionError("SSH failed", it) }
            val first = files.list(".")
            val root = first.path
            val entries = first.entries.toMutableList()
            var cursor = first.cursor
            while (cursor != null) {
                val more = files.list(root, cursor)
                assertTrue(more.entries.size <= 256)
                entries += more.entries; cursor = more.cursor
            }
            val fixture = arguments.getString("sftpFixtureDirectory")
            if (fixture != null) {
                var page = files.list(sftpChild(root, fixture))
                var count = page.entries.size
                assertNotNull("Fixture must exercise directory paging", page.cursor)
                while (page.cursor != null) { page = files.list(page.path, page.cursor); count += page.entries.size }
                assertTrue(count > 256)
            }
            val testDirectory = sftpChild(root, "pebrel-test-${System.nanoTime()}")
            files.mkdir(testDirectory)
            val readme = sftpChild(testDirectory, "README.md")
            val text = "# SFTP\n\n中文与 emoji 🌌\n".toByteArray(Charsets.UTF_8)
            files.upload(readme, ByteArrayInputStream(text), text.size.toLong())
            val sample = files.preview(readme)
            assertTrue(sample.bytes.toString(Charsets.UTF_8).contains("SFTP"))
            assertArrayEquals(text, sample.bytes)
            val bytes = ByteArray(1024 * 1024 + 19) { (it % 251).toByte() }
            val destination = sftpChild(testDirectory, "roundtrip.bin")
            var terminalChecked = false
            files.upload(destination, ByteArrayInputStream(bytes), bytes.size.toLong()) { count, _ ->
                if (!terminalChecked && count >= 65536) {
                    assertTrue(terminal.sendText("printf '\\123FTP_PARALLEL_OK\\n'\r"))
                    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
                    while (terminal.frame?.rows?.any { it?.text?.contains("SFTP_PARALLEL_OK") == true } != true && System.nanoTime() < deadline) Thread.sleep(25)
                    assertTrue("SFTP held the shell output loop", terminal.frame?.rows?.any { it?.text?.contains("SFTP_PARALLEL_OK") == true } == true)
                    terminalChecked = true
                }
            }
            assertTrue(terminalChecked)
            val copy = ByteArrayOutputStream()
            val uploaded = files.download(destination, copy)
            assertArrayEquals(MessageDigest.getInstance("SHA-256").digest(bytes), MessageDigest.getInstance("SHA-256").digest(copy.toByteArray()))
            assertEquals("SFTP_EXISTS", (runCatching { files.upload(destination, ByteArrayInputStream(byteArrayOf(1)), 1) }.exceptionOrNull() as NativeSshException).code)
            val renamed = "$destination.renamed"
            files.rename(uploaded, renamed)
            files.remove(files.stat(renamed))
            val folder = sftpChild(testDirectory, "directory")
            files.mkdir(folder)
            files.upload(sftpChild(folder, "keep.txt"), ByteArrayInputStream(byteArrayOf(1)), 1)
            assertTrue(runCatching { files.remove(files.stat(folder)) }.isFailure)
            files.remove(files.stat(sftpChild(folder, "keep.txt")))
            files.remove(files.stat(folder))
            val started = CompletableDeferred<Unit>()
            val cancelPath = sftpChild(testDirectory, "cancel.bin")
            val upload = launch(Dispatchers.IO) {
                files.upload(cancelPath, ByteArrayInputStream(bytes), bytes.size.toLong()) { _, _ ->
                    started.complete(Unit)
                    Thread.sleep(50)
                }
            }
            started.await(); upload.cancelAndJoin()
            assertTrue(runCatching { files.stat(cancelPath) }.isFailure)
            assertFalse(files.list(testDirectory).entries.any { it.name.startsWith(".pebrel-upload-") })
            assertTrue(terminal.sendText("printf '\\123FTP_AFTER_CANCEL_OK\\n'\r"))
            withTimeout(5000) { while (terminal.frame?.rows?.any { it?.text?.contains("SFTP_AFTER_CANCEL_OK") == true } != true) delay(25) }
            files.remove(files.stat(readme))
            files.remove(files.stat(testDirectory))
        } finally { terminal.finishIfRunning() }
    }

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
