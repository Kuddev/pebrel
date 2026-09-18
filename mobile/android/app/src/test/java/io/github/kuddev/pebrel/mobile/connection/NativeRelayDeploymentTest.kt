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
    @Test fun managerFailuresAreNotCollapsedAndUnknownOutputStaysPrivate() {
        for (code in listOf("supported_init_required", "openrc_supervisor_required", "service_command_failed", "configuration_directory_not_empty")) {
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
        val stream = ("{\"event\":\"progress\",\"stage\":\"starting\"}\n" + access().toString(2) +
            "\n{\"event\":\"progress\",\"stage\":\"private-server-output\"}\n" +
            "{\"installed\":true,\"running\":true,\"ready\":false,\"configuration_retained\":true}\n").byteInputStream()
        val results = NativeRelayDeployment.readMessages(stream) { updates += it }
        assertEquals(listOf("starting"), updates)
        assertEquals(2, results.size)
        assertFalse(NativeRelayDeployment.parseState(results.last()).ready)
    }
    @Test fun unboundedRemoteOutputIsRejected() {
        assertThrows(java.io.IOException::class.java) {
            NativeRelayDeployment.readMessages(("x".repeat(9000) + "\n").byteInputStream()) {}
        }
    }
}
