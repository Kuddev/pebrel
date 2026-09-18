package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class NativeRelayDeploymentTest {
    @Test fun fourStepsKeepTheExactFailureAndIgnoreLateOrRegressiveProgress() {
        var progress = RelayInstallProgress()
        assertEquals(InstallStepState.ACTIVE, progress.state(1))
        assertEquals(InstallStepState.WAITING, progress.state(4))
        progress = progress.advance(RelayServiceProgress("checking"))
            .advance(RelayServiceProgress("uploading", 32768, 65536))
        assertEquals(InstallStepState.DONE, progress.state(2))
        assertEquals(3, progress.step)
        assertEquals(progress, progress.advance(RelayServiceProgress("checking")))
        assertEquals(progress, progress.advance(RelayServiceProgress("uploading", 0, 65536)))
        val failed = progress.copy(failed = true)
        assertEquals(InstallStepState.FAILED, failed.state(3))
        assertEquals(InstallStepState.WAITING, failed.state(4))
        assertEquals(failed, failed.advance(RelayServiceProgress("ready")))
        val uploaded = progress.advance(RelayServiceProgress("uploaded", 65536, 65536))
        assertEquals(3, uploaded.step) // Remote hash verified; not yet installed.
        assertEquals(uploaded, uploaded.advance(RelayServiceProgress("uploading", 65536, 65536)))
        val verifying = uploaded.advance(RelayServiceProgress("verifying"))
        assertEquals(InstallStepState.ACTIVE, verifying.state(4))
        assertEquals(InstallStepState.CANCELLED, verifying.copy(cancelled = true).state(4))
        assertTrue((1..4).all { verifying.copy(finished = true).state(it) == InstallStepState.DONE })
    }

    @Test fun uploadReportsOnlyWrittenBytesAndStopsCountingOnFailure() {
        val bytes = ByteArray(70000) { (it % 251).toByte() }
        val written = java.io.ByteArrayOutputStream()
        val counts = mutableListOf<Int>()
        NativeRelayDeployment.writeUpload(written, bytes) { sent, total ->
            assertEquals(bytes.size, total)
            assertEquals(written.size(), sent)
            counts += sent
        }
        assertArrayEquals(bytes, written.toByteArray())
        assertEquals(listOf(32768, 65536, 70000), counts)
        counts.clear()
        val broken = object : java.io.OutputStream() {
            override fun write(value: Int) = throw java.io.IOException("closed")
        }
        assertThrows(java.io.IOException::class.java) {
            NativeRelayDeployment.writeUpload(broken, bytes) { sent, _ -> counts += sent }
        }
        assertTrue(counts.isEmpty())
    }

    @Test fun nativeJsonReadsAreBufferedWithoutChangingFrameLimits() {
        var reads = 0
        val bytes = ("{\"event\":\"progress\",\"stage\":\"starting\"}\n".repeat(100)).byteInputStream()
        val input = object : java.io.InputStream() {
            override fun read(): Int = error("must use bulk native reads")
            override fun read(b: ByteArray, off: Int, len: Int): Int { reads++; return bytes.read(b, off, len) }
        }
        var stages = 0
        NativeRelayDeployment.readMessages(input) { stages++ }
        assertEquals(100, stages)
        assertTrue(reads <= 3)
    }

    @Test fun managerFailuresAreNotCollapsedAndUnknownOutputStaysPrivate() {
        for (code in listOf("systemd_239_required", "supported_init_required", "openrc_supervisor_required", "service_command_failed", "configuration_directory_not_empty")) {
            val error = assertThrows(RelayServiceFailure::class.java) {
                NativeRelayDeployment.checkedMessages(1, emptyList(), listOf(JSONObject().put("error", code)))
            }
            assertEquals(code, error.code)
        }
        val error = assertThrows(RelayServiceFailure::class.java) {
            NativeRelayDeployment.checkedMessages(1, emptyList(), listOf(JSONObject().put("error", "private credentials")))
        }
        assertEquals("service_failed", error.code)
        assertTrue(NativeRelayDeployment.preflightCommand().contains("/sbin/openrc-run"))
        assertTrue(NativeRelayDeployment.preflightCommand().contains("supported_init_required"))
    }
    private fun access() = JSONObject().put("version", 2).put("url", "wss://192.0.2.10:443")
        .put("room", "r".repeat(43)).put("tlsPin", "sha256/" + "A".repeat(43) + "=")
        .put("desktopToken", "a".repeat(43)).put("mobileToken", "b".repeat(43))

    @Test fun ipDeploymentDoesNotRequireDomainAndRejectsShellOrUrlInputs() {
        assertEquals("192.0.2.10", NativeRelayDeployment.validatedAddress("192.0.2.10"))
        assertEquals("2001:db8::1", NativeRelayDeployment.validatedAddress("[2001:db8::1]"))
        listOf("wss://host", "host;touch /tmp/no", "user@host", "-flag", "").forEach {
            assertThrows(RelayServiceFailure::class.java) { NativeRelayDeployment.validatedAddress(it) }
        }
    }
    @Test fun exportIsDesktopAccessNotAPhoneInvitation() {
        NativeRelayDeployment.validateAccess(access())
        assertThrows(RelayServiceFailure::class.java) { NativeRelayDeployment.validateAccess(access().put("version", 1)) }
        assertThrows(RelayServiceFailure::class.java) { NativeRelayDeployment.validateAccess(access().put("url", "wss://user:pass@host")) }
        assertThrows(RelayServiceFailure::class.java) { NativeRelayDeployment.validateAccess(access().put("desktopToken", "b".repeat(43))) }
    }
    @Test fun onlyRealAllowlistedStagesReachProgressAndExportsRemainPrivate() {
        val updates = mutableListOf<String>()
        val stream = ("{\"event\":\"progress\",\"stage\":\"uploaded\"}\n" +
            "{\"event\":\"progress\",\"stage\":\"starting\"}\n" + access().toString(2) +
            "\n{\"event\":\"progress\",\"stage\":\"private-server-output\"}\n" +
            "{\"installed\":true,\"running\":true,\"ready\":false,\"configuration_retained\":true}\n").byteInputStream()
        val results = NativeRelayDeployment.readMessages(stream) { updates += it }
        assertEquals(listOf("uploaded", "starting"), updates)
        assertEquals(2, results.size)
        assertFalse(NativeRelayDeployment.parseState(results.last()).ready)
    }
    @Test fun unboundedRemoteOutputIsRejected() {
        assertThrows(java.io.IOException::class.java) {
            NativeRelayDeployment.readMessages(("x".repeat(9000) + "\n").byteInputStream()) {}
        }
    }
}
