package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.Base64

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
@OptIn(ExperimentalCoroutinesApi::class)
class RelayRoutesTest {
    private fun key(value: Int) = Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32) { value.toByte() })
    private fun profile(mode: String, room: String = mode, host: Int = 1, invitation: Boolean = false) = RelayProfile(
        if (mode == "lan") "wss://192.0.2.1:4000" else "wss://relay.example.com", room, key(2), "Computer",
        "sha256/${Base64.getEncoder().encodeToString(ByteArray(32) { 3 })}", mode,
        SecureRelayProfile(key(host), "grant-$room", key(4), invitation, if (invitation) 9999999999 else 0))

    @Test fun eitherPairingOrderKeepsOneComputerAndBothRoutesAcrossStorage() {
        for (order in listOf(listOf("lan", "relay"), listOf("relay", "lan"))) {
            val originals = order.map { profile(it) }
            val saved = RelayProfile.latestByComputer(originals).single()
            val restored = RelayProfile.parseStored(saved.toStoredJson().toString())
            assertEquals(saved.id, restored.id)
            assertEquals(listOf("lan", "relay"), restored.connectionRoutes().map { it.mode })
            restored.connectionRoutes().forEach { route ->
                assertTrue(route.sameConnection(originals.single { it.mode == route.mode }))
            }
            assertEquals(saved.toStoredJson().toString(), restored.toStoredJson().toString())
            assertNull(restored.alternate!!.alternate)
        }
    }

    @Test fun replacingOneRoutePreservesTheOtherAndNeverGrowsAnAlternateChain() {
        val saved = RelayProfile.latestByComputer(listOf(profile("lan"), profile("relay"))).single()
        val replacement = profile("relay", "rotated")
        val updated = RelayProfile.latestByComputer(listOf(saved, replacement)).single()
        assertSame(replacement, updated)
        assertEquals("rotated", updated.device)
        assertEquals("lan", updated.alternate!!.device)
        val again = RelayProfile.latestByComputer(listOf(updated, profile("lan", "new-lan"))).single()
        assertEquals(listOf("new-lan", "rotated"), again.connectionRoutes().map { it.device })
        assertNull(again.alternate!!.alternate)
    }

    @Test fun legacyRecordsStillLoadAndInvitationsCannotSupplyAnotherSavedRoute() {
        val legacy = RelayProfile("wss://relay.example.com", "old", key(2), "Computer")
        assertTrue(legacy.sameConnection(RelayProfile.parseStored(legacy.toJson().toString())))
        val saved = RelayProfile.latestByComputer(listOf(profile("lan"), profile("relay"))).single()
        assertNull(RelayProfile.parse(saved.toStoredJson().toString()).alternate)
        val invitation = profile("lan", invitation = true)
        invitation.retainRoutes(saved)
        assertNull(invitation.alternate)
        assertThrows(IllegalArgumentException::class.java) {
            RelayProfile.parseStored(invitation.toJson().put("alternate", saved.toJson()).toString())
        }
    }

    @Test fun differentComputersAndInvalidStoredAlternatesCannotMerge() {
        val lan = profile("lan")
        val other = profile("relay", host = 9)
        assertEquals(2, RelayProfile.latestByComputer(listOf(lan, other)).size)
        assertNull(lan.alternate)
        for (alternate in listOf(other.toJson(), profile("lan", "duplicate").toJson(),
            profile("relay").toJson().put("alternate", JSONObject()))) {
            assertThrows(IllegalArgumentException::class.java) {
                RelayProfile.parseStored(lan.toJson().put("alternate", alternate).toString())
            }
        }
    }

    @Test fun aSavedConnectionAlwaysTriesLanFirstAndFallsBackOnlyOnce() {
        val saved = RelayProfile.latestByComputer(listOf(profile("lan"), profile("relay"))).single()
        for (requested in saved.connectionRoutes()) {
            val attempt = RelayRouteAttempt.forProfile(requested, saved)
            assertEquals("lan", attempt.next()!!.mode)
            assertTrue(attempt.hasFallback)
            assertEquals("relay", attempt.next(DesktopFailureKind.TIMEOUT)!!.mode)
            assertFalse(attempt.hasFallback)
            assertNull(attempt.next(DesktopFailureKind.NETWORK))
        }
    }

    @Test fun certificateAuthenticationAndProtocolFailuresStopFallback() {
        val saved = RelayProfile.latestByComputer(listOf(profile("lan"), profile("relay"))).single()
        for (failure in listOf(DesktopFailureKind.CERTIFICATE_CHANGED, DesktopFailureKind.CERTIFICATE_DATE,
            DesktopFailureKind.TLS, DesktopFailureKind.AUTHENTICATION, DesktopFailureKind.PROTOCOL)) {
            val attempt = RelayRouteAttempt.forProfile(saved, saved)
            assertEquals("lan", attempt.next()!!.mode)
            assertNull(attempt.next(failure))
            assertNull(attempt.next())
        }
    }

    @Test fun newInvitationsAndEditedAddressesMustAuthenticateBeforeUsingSavedRoutes() {
        val saved = RelayProfile.latestByComputer(listOf(profile("lan"), profile("relay"))).single()
        for (requested in listOf(profile("relay", "new", invitation = true),
            saved.connectionRoutes().first().atLanAddress("192.0.2.2:4000"))) {
            val attempt = RelayRouteAttempt.forProfile(requested, saved)
            assertSame(requested, attempt.next())
            assertNull(attempt.next(DesktopFailureKind.NETWORK))
        }
    }

    @Test fun aLanDeadlineClosesEachStalledHandshakeBeforeAllowingFallback() = runTest {
        for (stage in 0..3) {
            val closed = CompletableDeferred<Unit>()
            val failures = mutableListOf<DesktopFailureKind>()
            val transport = object : DesktopTransport {
                lateinit var receive: (JSONObject) -> Unit
                override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit,
                                          disconnected: (Throwable?) -> Unit) {
                    this.receive = receive
                    if (stage == 0) awaitCancellation()
                    if (stage >= 2) receive(JSONObject().put("type", "mobile.ready")
                        .put("protocol", "pebrel.mobile.relay").put("version", 1))
                }
                override fun send(frame: JSONObject) {
                    if (stage == 3) receive(JSONObject().put("id", frame.getString("id"))
                        .put("ok", true).put("result", JSONObject()))
                }
                override fun close() { closed.complete(Unit) }
            }
            val client = DesktopRuntimeClient(transport, { fail("a stalled route is not ready") }, failures::add)
            val started = currentTime
            val result = runCatching { withTimeout(5_000) { client.connect(true) } }
            assertTrue("stage $stage", result.exceptionOrNull() is DesktopConnectionFailure)
            assertEquals(5_000L, currentTime - started)
            assertEquals(1, failures.size)
            assertTrue(DesktopReconnect.retryable(failures.single()))
            closed.await()
            val attempt = RelayRouteAttempt(listOf(profile("lan"), profile("relay")))
            assertEquals("lan", attempt.next()!!.mode)
            assertEquals("relay", attempt.next(failures.single())!!.mode)
            assertNull(attempt.next())
        }
    }

    @Test fun cancellingAnAttemptClosesItWithoutReportingARetryableFailure() = runTest {
        val closed = CompletableDeferred<Unit>()
        val failures = mutableListOf<DesktopFailureKind>()
        val transport = object : DesktopTransport {
            override suspend fun open(allowInput: Boolean, receive: (JSONObject) -> Unit,
                                      disconnected: (Throwable?) -> Unit) { awaitCancellation() }
            override fun send(frame: JSONObject) { fail("cancelled handshake cannot send") }
            override fun close() { closed.complete(Unit) }
        }
        val client = DesktopRuntimeClient(transport, {}, failures::add)
        val attempt = launch { client.connect(true) }
        runCurrent()
        attempt.cancelAndJoin()
        closed.await()
        assertTrue(failures.isEmpty())
    }
}
