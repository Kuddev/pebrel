package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.Base64

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class SecureRelaySessionTest {
    private fun key(value: Int) = Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32) { value.toByte() })
    private fun profile(invitation: Boolean = true) = RelayProfile("wss://fixture.invalid", "room", key(1), "PC",
        "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "relay", SecureRelayProfile(key(2), "grant", key(3), invitation, 400))

    /** Lifecycle fake, not a cryptographic implementation/test. Real Noise is
     * exercised by the Rust TLS relay integration tests. */
    private class Cipher : RelayCipher {
        var closed = false
        override fun hello() = byteArrayOf(99)
        override fun finish(bytes: ByteArray) { require(bytes.contentEquals(byteArrayOf(88))) }
        override fun seal(bytes: ByteArray) = arrayOf(bytes.copyOf())
        override fun open(bytes: ByteArray) = bytes.copyOf()
        override fun close() { closed = true }
    }

    @Test fun v2ProfileRoundTripsWithoutLosingSecurityOrChangingIdentityOnEnrollment() {
        val profile = profile()
        val restored = RelayProfile.parse(profile.toJson().toString())
        assertEquals(2, restored.version)
        assertEquals(profile.id, restored.id)
        val identity = restored.id
        restored.secure = SecureRelayProfile(key(2), "device-grant", key(4), false)
        assertEquals(identity, RelayProfile.parse(restored.toJson().toString()).id)
        val different = profile.toJson().apply { getJSONObject("secure").put("host", key(5)) }
        assertNotEquals(identity, RelayProfile.parse(different.toString()).id)
        assertFalse(profile.toString().contains(key(3)))
        assertFalse(restored.secure.toString().contains(key(4)))
    }

    @Test fun downgradeMissingPinAndNonCanonicalSecretsAreRejected() {
        val wrongVersion = profile().toJson().put("version", 1)
        assertThrows(IllegalArgumentException::class.java) { RelayProfile.parse(wrongVersion.toString()) }
        val missingPin = profile().toJson().apply { remove("tlsPin") }
        assertThrows(IllegalArgumentException::class.java) { RelayProfile.parse(missingPin.toString()) }
        val badSecret = profile().toJson().apply { getJSONObject("secure").put("secret", "a".repeat(43)) }
        assertThrows(IllegalArgumentException::class.java) { RelayProfile.parse(badSecret.toString()) }
    }

    @Test fun encryptedEnrollmentRotatesTheQrSecretBeforeRuntimeIsAllowed() {
        val profile = profile()
        val sent = mutableListOf<ByteArray>()
        val received = mutableListOf<JSONObject>()
        val cipher = Cipher()
        var context = ""
        val session = SecureRelaySession(profile, { sent.add(it) }, { received.add(it) }, { _, _, transcript ->
            context = transcript; cipher
        }, { 101 })
        assertThrows(IllegalStateException::class.java) { session.runtime(JSONObject()) }
        session.paired("epoch")
        assertEquals("pebrel.mobile.v2\n${key(2)}\ngrant\ninvite\nepoch", context)
        assertEquals(1.toByte(), sent[0][0])
        assertFalse(String(sent[0]).contains(key(3)))
        session.binary(byteArrayOf(88))
        assertEquals("secure.connect", JSONObject(String(sent.last())).getString("type"))
        session.binary(JSONObject().put("type", "secure.enrolled").put("grant", "new-device")
            .put("secret", key(4)).toString().toByteArray())
        assertEquals("secure.ack", JSONObject(String(sent.last())).getString("type"))
        assertEquals("new-device", profile.secure!!.grant)
        assertFalse(profile.secure!!.invitation)
        assertTrue(received.isEmpty())
        session.binary("{\"type\":\"mobile.ready\"}".toByteArray())
        assertEquals("mobile.ready", received.single().getString("type"))
        session.runtime(JSONObject().put("id", "once"))
        session.close()
        assertTrue(cipher.closed)
        assertThrows(IllegalStateException::class.java) { session.runtime(JSONObject()) }
    }

    @Test fun expiredQrNeverStartsCryptoAndASecondPeerCannotReuseTheChannel() {
        var created = 0
        val expired = SecureRelaySession(profile(), {}, {}, { _, _, _ -> created++; Cipher() }, { 400 })
        assertThrows(IllegalArgumentException::class.java) { expired.paired("epoch") }
        assertEquals(0, created)
        val active = SecureRelaySession(profile(), {}, {}, { _, _, _ -> Cipher() }, { 101 })
        active.paired("first")
        assertThrows(IllegalStateException::class.java) { active.paired("second") }
        active.close()
    }

    @Test fun anExistingDeviceRejectsChangedGrantAndNoPlaintextReadyIsPublished() {
        val received = mutableListOf<JSONObject>()
        val session = SecureRelaySession(profile(false), {}, { received.add(it) }, { _, _, _ -> Cipher() })
        session.paired("epoch")
        session.binary(byteArrayOf(88))
        assertThrows(IllegalStateException::class.java) {
            session.binary("{\"type\":\"secure.accepted\",\"grant\":\"other\"}".toByteArray())
        }
        assertTrue(received.isEmpty())
        session.close()
    }
}
