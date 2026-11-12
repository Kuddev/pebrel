package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.*
import org.junit.Test

class SshPtyIdentityTest {
    @Test fun anExecChannelFindsOnlyThePtyUnderItsOwnSshSessionProcess() {
        val processes = """
            120
            1 0 ? init
            10 1 ? sshd
            100 10 ? sshd
            110 100 pts/7 bash
            111 110 pts/7 tmux: client
            120 100 ? sh
            121 120 ? ps
            200 10 ? sshd
            210 200 pts/9 zsh
            211 210 pts/9 tmux: client
            300 1 ? tmux: server
            310 300 pts/11 bash
        """.trimIndent()
        assertEquals("/dev/pts/7", SshPtyIdentity.parse(processes))
        assertEquals("/dev/pts/9", SshPtyIdentity.parse(processes.replaceFirst("120\n", "210\n")))
    }

    @Test fun multiplePtysOnOneConnectionAndUnknownServersAreNeverGuessed() {
        val processes = "50\n1 0 ? init\n10 1 ? sshd\n20 10 pts/7 sh\n50 10 ? sh\n"
        assertEquals("/dev/pts/7", SshPtyIdentity.parse(processes))
        assertNull(SshPtyIdentity.parse(processes + "30 10 pts/8 sh\n"))
        assertNull(SshPtyIdentity.parse(processes.replace("sshd", "unknown-server")))
        assertNull(SshPtyIdentity.parse(processes.replace("50 10", "50 50")))
        assertNull(SshPtyIdentity.parse(processes.replace("pts/7", "?")))
        assertNull(SshPtyIdentity.parse(processes + "20 1 pts/8 sh\n"))
    }

    @Test fun knownOpenSshProcessNamesAndDarwinTtyPathsAreSupported() {
        val processes = "50\n1 0 ?? launchd\n10 1 ?? /usr/libexec/sshd-session\n20 10 ttys003 /bin/zsh\n50 10 ?? /bin/sh\n"
        assertEquals("/dev/ttys003", SshPtyIdentity.parse(processes))
        assertNull(SshPtyIdentity.parse(processes.replace("ttys003", "../../../tmp/wrong")))
        assertNull(SshPtyIdentity.parse("shell banner\n$processes"))
        assertNull(SshPtyIdentity.parse(processes + "x".repeat(65536)))
    }
}
