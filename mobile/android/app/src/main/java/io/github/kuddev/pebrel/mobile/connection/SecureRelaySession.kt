package io.github.kuddev.pebrel.mobile.connection

import io.github.kuddev.pebrel.ssh.SecureMobileLink
import org.json.JSONObject
import java.io.Closeable
import java.util.Base64

/** Never a data class: generated toString must not expose the QR/device secret. */
class SecureRelayProfile(val host: String, val grant: String, val secret: String,
                         val invitation: Boolean, val expiresAt: Long = 0) {
    fun toJson(): JSONObject = JSONObject().put("host", host).put("grant", grant)
        .put("secret", secret).put("invitation", invitation).put("expiresAt", expiresAt)
    override fun toString() = "SecureRelayProfile(redacted)"
    companion object {
        internal fun key(value: String): String {
            require(value.length == 43)
            val bytes = Base64.getUrlDecoder().decode(value)
            try { require(bytes.size == 32 && Base64.getUrlEncoder().withoutPadding().encodeToString(bytes) == value) }
            finally { bytes.fill(0) }
            return value
        }
        internal fun id(value: String): String {
            require(Regex("[A-Za-z0-9_-]{1,64}").matches(value))
            return value
        }
        fun parse(data: JSONObject): SecureRelayProfile {
            val invitation = data.getBoolean("invitation")
            val expires = data.optLong("expiresAt", 0)
            require(!invitation || expires > 0)
            return SecureRelayProfile(key(data.getString("host")), id(data.getString("grant")),
                key(data.getString("secret")), invitation, expires)
        }
    }
}

/** Injectable only to test lifecycle/wire contracts; production has one native
 * crypto authority and never falls back to a Kotlin cipher or plaintext. */
internal interface RelayCipher : Closeable {
    fun hello(): ByteArray
    fun finish(bytes: ByteArray)
    fun seal(bytes: ByteArray): Array<ByteArray>
    fun open(bytes: ByteArray): ByteArray?
}

private class NativeRelayCipher(host: String, secret: String, context: String) : RelayCipher {
    private val link = SecureMobileLink(host, secret, context)
    override fun hello() = link.hello()
    override fun finish(bytes: ByteArray) = link.finish(bytes)
    override fun seal(bytes: ByteArray) = link.seal(bytes)
    override fun open(bytes: ByteArray) = link.open(bytes)
    override fun close() = link.close()
}

internal class SecureRelaySession(
    private val profile: RelayProfile,
    private val send: (ByteArray) -> Unit,
    private val receive: (JSONObject) -> Unit,
    private val cipherFactory: (String, String, String) -> RelayCipher = ::NativeRelayCipher,
    private val now: () -> Long = { System.currentTimeMillis() / 1000 },
) : Closeable {
    private enum class Phase { NEW, HANDSHAKE, ENROLLMENT, READY, CLOSED }
    private var phase = Phase.NEW
    private var cipher: RelayCipher? = null
    private var credential: SecureRelayProfile? = null

    @Synchronized fun paired(epoch: String) {
        check(phase == Phase.NEW && profile.version == 2)
        SecureRelayProfile.id(epoch)
        val secure = checkNotNull(profile.secure)
        require(!secure.invitation || now() < secure.expiresAt)
        val context = "pebrel.mobile.v2\n${secure.host}\n${secure.grant}\n${if (secure.invitation) "invite" else "device"}\n$epoch"
        val link = cipherFactory(secure.host, secure.secret, context)
        cipher = link
        credential = secure
        phase = Phase.HANDSHAKE
        val header = JSONObject().put("host", secure.host).put("grant", secure.grant).put("invitation", secure.invitation)
        send(byteArrayOf(1) + header.toString().toByteArray(Charsets.UTF_8))
        send(link.hello())
    }

    @Synchronized fun binary(bytes: ByteArray) {
        require(bytes.size in 1..65535)
        val link = checkNotNull(cipher)
        when (phase) {
            Phase.HANDSHAKE -> {
                link.finish(bytes)
                phase = Phase.ENROLLMENT
                encrypted(JSONObject().put("type", "secure.connect").put("name", "Pebrel Android"))
            }
            Phase.ENROLLMENT, Phase.READY -> {
                val plain = link.open(bytes) ?: return
                try {
                    require(plain.size in 1..(2 * 1024 * 1024))
                    if (phase == Phase.ENROLLMENT) require(plain.size <= 1024)
                    val frame = JSONObject(String(plain, Charsets.UTF_8))
                    if (phase == Phase.READY) receive(frame) else enrolled(frame)
                } finally { plain.fill(0) }
            }
            else -> error("invalid_secure_link_state")
        }
    }

    private fun enrolled(frame: JSONObject) {
        val previous = checkNotNull(credential)
        val grant = SecureRelayProfile.id(frame.getString("grant"))
        if (previous.invitation) {
            check(frame.getString("type") == "secure.enrolled" && grant != previous.grant)
            val secret = SecureRelayProfile.key(frame.getString("secret"))
            require(secret != previous.secret)
            // Keep the rotated credential for a retry if Runtime startup fails.
            // The repository still saves a computer only after its first snapshot.
            profile.secure = SecureRelayProfile(previous.host, grant, secret, false)
        } else check(frame.getString("type") == "secure.accepted" && grant == previous.grant)
        encrypted(JSONObject().put("type", "secure.ack").put("grant", grant))
        phase = Phase.READY
    }

    @Synchronized fun runtime(frame: JSONObject) {
        check(phase == Phase.READY)
        require(frame.toString().toByteArray().size <= 40 * 1024)
        encrypted(frame)
    }
    private fun encrypted(frame: JSONObject) {
        val plain = frame.toString().toByteArray(Charsets.UTF_8)
        try { checkNotNull(cipher).seal(plain).forEach(send) }
        finally { plain.fill(0) }
    }
    @Synchronized override fun close() {
        phase = Phase.CLOSED
        cipher?.close()
        cipher = null
        credential = null
    }
}
