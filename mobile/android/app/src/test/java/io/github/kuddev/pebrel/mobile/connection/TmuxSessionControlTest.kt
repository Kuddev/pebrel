package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.*
import java.io.ByteArrayInputStream

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = android.app.Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class TmuxSessionControlTest {
    @Test fun nativeWheelBurstsNeverQueryTmuxAndTheNextInputLeavesCopyModeOnce() {
        val commands = mutableListOf<String>()
        val controller = TmuxSessionControl({ command ->
            commands += command
            JSONObject().put("status", 0).put("stdout", if (command.contains("list-clients"))
                "/dev/pts/7\t\$3\tWork\t10:20\t@4\tEditor\t%5\tcopy-mode\n" else "")
        }, {})
        val token = Regex("PEBREL_TTY_([a-f0-9]+)=").find(controller.command("true"))!!.groupValues[1]
        controller.input(ByteArrayInputStream("\u001ePEBREL_TTY_$token=/dev/pts/7\u001f".toByteArray())).readBytes()
        repeat(500) { controller.applicationScrolled() }
        assertTrue(commands.isEmpty())
        controller.returnToInput()
        assertEquals(2, commands.size)
        assertTrue(commands.last().contains("cancel"))
        controller.returnToInput()
        assertEquals(2, commands.size)
    }

    @Test fun anotherMouseApplicationDoesNotRequireATmuxClientBeforeTyping() {
        val controller = TmuxSessionControl({ error("no known remote tty") }, {}, detectShell = true)
        controller.applicationScrolled()
        controller.returnToInput()
    }

    @Test fun touchScrollUsesTheCurrentClientsCopyModeAndInputCancelsItWithoutSendingShellKeys() {
        val commands = mutableListOf<String>()
        val targets = mutableListOf<RemoteAttachment?>()
        var mode = ""
        var window = "@4"
        val controller = TmuxSessionControl({ command ->
            commands += command
            JSONObject().put("status", 0).put("stdout", if (command.contains("list-clients"))
                "/dev/pts/other\t${'$'}99\tOther\t10:20\t@99\tOther\t%99\t\n" +
                "/dev/pts/7\t${'$'}3\tWork\t10:20\t$window\tEditor\t%5\t$mode\n" else "")
        }, targets::add)
        val wrapped = controller.command("exec tmux attach-session -t 'work'")
        val token = Regex("PEBREL_TTY_([a-f0-9]+)=").find(wrapped)!!.groupValues[1]
        controller.input(ByteArrayInputStream("\u001ePEBREL_TTY_$token=/dev/pts/7\u001foutput".toByteArray())).readBytes()
        controller.scroll(-3)
        assertTrue(commands.last().contains("copy-mode"))
        assertTrue(commands.last().contains("scroll-up"))
        assertTrue(commands.last().contains("-N 3"))
        assertFalse(commands.last().contains("%99"))
        mode = "copy-mode"
        controller.returnToInput()
        assertTrue(commands.last().contains("cancel"))
        assertTrue(commands.last().contains("${'$'}3:@4.%5"))
        window = "@8"
        assertEquals("@8", controller.checkpoint()?.window?.id)
        assertEquals("@8", targets.last()?.window?.id)
    }

    @Test fun noMarkerOrAnotherClientCannotSelectAnArbitraryTmuxSession() {
        var queried = false
        val controller = TmuxSessionControl({ queried = true; error("no tty") }, {})
        controller.scroll(-1)
        assertFalse(queried)
        assertNull(TmuxSessionControl.parseClient("/dev/pts/other\t${'$'}3\tWork\t10:20\t@4\tEditor\t%5\t", "/dev/pts/7"))
        assertNull(TmuxSessionControl.parseClient("/dev/pts/7\t${'$'}3\tWork\t10:20\t@4\tEditor\t%5;bad\t", "/dev/pts/7"))
    }

    @Test fun manuallyEnteredTmuxIsTrackedAndIntentionalDetachRestoresNormalShellInput() {
        val commands = mutableListOf<String>()
        val targets = mutableListOf<RemoteAttachment?>()
        var attached = true
        var mode = ""
        val controller = TmuxSessionControl({ command ->
            commands += command
            val output = when {
                command == SshPtyIdentity.command -> "50\n1 0 ? init\n10 1 ? sshd\n20 10 pts/7 sh\n50 10 ? sh\n"
                command.contains("list-clients") -> "/dev/pts/9\t${'$'}9\tOther\t10:20\t@9\tOther\t%9\t\n" +
                    if (attached) "/dev/pts/7\t${'$'}3\tWork\t10:20\t@4\tEditor\t%5\t$mode\n" else ""
                else -> ""
            }
            JSONObject().put("status", 0).put("stdout", output)
        }, targets::add, detectShell = true)
        assertFalse(controller.scroll(-1))
        assertTrue(commands.isEmpty())
        val bytes = "\u001eopaque shell output\u001f".toByteArray()
        assertArrayEquals(bytes, controller.input(ByteArrayInputStream(bytes)).readBytes())
        controller.identifyShellTerminal()
        assertEquals("${'$'}3", targets.last()?.session?.id)
        assertTrue(controller.scroll(-2))
        assertTrue(commands.last().contains("scroll-up"))
        mode = "copy-mode"
        controller.returnToInput()
        assertTrue(commands.last().contains("cancel"))
        controller.scroll(-2)
        attached = false
        controller.returnToInput() // Detach while reading must not leave input permanently blocked.
        assertNull(targets.last())
        assertFalse(controller.scroll(-2))
        controller.identifyShellTerminal()
        assertTrue(commands.count { it == SshPtyIdentity.command } >= 2)
    }

    @Test fun namedServerUsesItsExecutableAndSocketForCheckpointScrollAndReconnect() {
        val endpoint = TmuxEndpoint("/opt/server 3.8/bin/tmux", "/tmp/tmux-123/custom socket")
        val commands = mutableListOf<String>()
        val targets = mutableListOf<RemoteAttachment?>()
        var present = true
        val processes = "50\n1 0 ? init\n10 1 ? sshd\n20 10 pts/7 sh\n21 20 pts/7 tmux: client\n50 10 ? sh\n"
        val controller = TmuxSessionControl({ command ->
            commands += command
            val output = when {
                command == SshPtyIdentity.command -> if (present) processes else "50\n1 0 ? init\n10 1 ? sshd\n50 10 ? sh\n"
                command == TmuxEndpoint.discoveryCommand -> "${endpoint.executable}\t${endpoint.socket}\n"
                command.contains("list-clients") && command.contains(endpoint.socket) && present ->
                    "/dev/pts/7\t\$3\tWork\t10:20\t@4\tEditor\t%5\t\n"
                else -> ""
            }
            JSONObject().put("status", 0).put("stdout", output)
        }, targets::add, detectShell = true)
        controller.identifyShellTerminal()
        assertEquals(endpoint, targets.last()!!.session.endpoint)
        commands.clear()
        assertTrue(controller.scroll(-2))
        assertEquals(2, commands.size)
        assertTrue(commands.all { it.contains(endpoint.executable) && it.contains(endpoint.socket) })
        val resume = RemoteSessions.attachCommand(targets.last()!!)
        assertTrue(resume.contains(endpoint.executable) && resume.contains(endpoint.socket))
        assertTrue(resume.contains("\$3:@4.%5"))
        assertTrue(resume.contains("SHELL:-/bin/sh"))
        val count = targets.size
        present = false
        assertNull(controller.checkpoint())
        assertEquals("A disappearing PTY is not an intentional detach", count, targets.size)
    }

    @Test fun malformedEndpointsCannotBecomeShellCommands() {
        assertTrue(TmuxEndpoint.parse("bad;command\t/tmp/socket\n/bin/remote\trelative\n").isEmpty())
        assertThrows(IllegalArgumentException::class.java) { TmuxEndpoint("bad;command", "/tmp/s").command() }
        val endpoint = TmuxEndpoint("/opt/it's executable", "/tmp/it's socket")
        assertTrue(endpoint.command().contains("'\"'\"'"))
    }

    @Test fun ambiguousShellDiscoveryLeavesOtherTerminalApplicationsUntouched() {
        val commands = mutableListOf<String>()
        val controller = TmuxSessionControl({ command ->
            commands += command
            JSONObject().put("status", 0).put("stdout",
                "50\n1 0 ? init\n10 1 ? sshd\n20 10 pts/7 sh\n21 10 pts/8 sh\n50 10 ? sh\n")
        }, { fail("ambiguous process ownership cannot select an attachment") }, detectShell = true)
        controller.identifyShellTerminal()
        assertNull(controller.checkpoint())
        assertFalse(controller.scroll(-3))
        controller.returnToInput()
        assertEquals(listOf(SshPtyIdentity.command), commands)
    }
}
