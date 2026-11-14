package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.TimeUnit

class WorkspaceRecoveryCommandTest {
    @Test fun reconnectAndBrowseDoNotCreateMissingSessionsOrUseAnotherHost() {
        assumeTrue(File("/bin/sh").canExecute())
        val directory = Files.createTempDirectory("pebrel-workspace-resume-").toFile()
        try {
            val mode = SshSessionMode.HERDR
            val host = HostProfile("host", "SSH", "localhost", user = "user", sessionMode = mode, sessionName = "work.dev")
            val session = RemoteSession("herdr", "work.dev", "Work")
            val selected = RemoteAttachment(session, RemoteWindow("w1:t2", "Editor"))
            val executable = directory.resolve(mode.id)
            executable.writeText("""
                #!/bin/sh
                if [ "${'$'}1" = --session ]; then
                    name=${'$'}2
                    shift 2
                else
                    name=${'$'}{HERDR_SESSION:-default}
                fi
                case "${'$'}1:${'$'}2" in
                    session:attach)
                        [ -f "${'$'}3" ] || printf w1:t1 > "${'$'}3"
                        printf '%s:%s' "${'$'}3" "${'$'}(cat "${'$'}3")"
                        ;;
                    tab:focus) [ -f "${'$'}name" ] && [ "${'$'}3" = w1:t2 ] && printf '%s' "${'$'}3" > "${'$'}name" ;;
                    client:)
                        if [ "${'$'}HERDR_REMOTE_KEYBINDINGS" != server ]; then
                            touch wrong-host
                            exit 1
                        fi
                        [ -f "${'$'}name" ] && printf '%s:%s' "${'$'}name" "${'$'}(cat "${'$'}name")"
                        ;;
                    *) exit 2 ;;
                esac
            """.trimIndent())
            check(executable.setExecutable(true))
            fun run(command: String?): Pair<Int, String> {
                val child = ProcessBuilder("/bin/sh", "-c", checkNotNull(command)).directory(directory).apply {
                    environment().apply {
                        put("PATH", "${directory.absolutePath}:/usr/bin:/bin")
                        put("HOME", directory.absolutePath)
                        put("HERDR_SESSION", "work.dev")
                        put("HERDR_REMOTE_KEYBINDINGS", "local")
                    }
                }.start()
                check(child.waitFor(5, TimeUnit.SECONDS)) { child.destroyForcibly(); "attach did not finish" }
                return child.exitValue() to child.inputStream.bufferedReader().use { it.readText() }
            }
            val retry = host.reconnectCommand()
            val browse = RemoteSessions.attachCommand(RemoteAttachment(session))
            val focus = RemoteSessions.attachCommand(selected)

            assertEquals(0, run(host.attachCommand()).first)
            assertEquals(0 to "work.dev:w1:t1", run(retry))
            assertEquals(0 to "work.dev:w1:t2", run(focus))
            assertEquals(0 to "work.dev:w1:t2", run(browse))
            assertNotEquals(0, run(RemoteSessions.attachCommand(selected.copy(window = RemoteWindow("w1:gone", "Gone")))).first)

            fun assertMissing(command: String?) {
                assertNotEquals(0, run(command).first)
                assertFalse(directory.resolve("work.dev").exists())
                assertFalse(directory.resolve("default").exists())
                assertFalse(directory.resolve("wrong-host").exists())
            }
            check(directory.resolve("work.dev").delete())
            for (command in listOf(retry, browse, focus)) assertMissing(command)

            val inherited = host.copy(sessionName = "").reconnectCommand()
            assertEquals(0, run(host.attachCommand()).first)
            assertEquals(0 to "work.dev:w1:t1", run(inherited))
            check(directory.resolve("work.dev").delete())
            assertMissing(inherited)
        } finally {
            directory.deleteRecursively()
        }
    }
}
