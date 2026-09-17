package io.github.kuddev.pebrel.mobile.connection

import okhttp3.HttpUrl.Companion.toHttpUrl
import org.json.JSONObject
import java.security.MessageDigest

/** Persisted invitations share the same wire contract for LAN and relayed connections. */
class RelayProfile(val url: String, val device: String, val token: String, val name: String,
                   val tlsPin: String? = null, val mode: String = "relay", secure: SecureRelayProfile? = null) {
    @Volatile var secure: SecureRelayProfile? = secure
        internal set
    val version: Int = if (secure == null) 1 else 2
    val id: String = MessageDigest.getInstance("SHA-256").digest(("$url/$device" + (tlsPin?.let { "/$it" } ?: "") + (secure?.let { "/${it.host}" } ?: "")).toByteArray())
        .joinToString("") { "%02x".format(it) }
    fun toJson(): JSONObject = JSONObject().put("version", version).put("url", url).put("device", device).put("token", token).put("name", name).put("mode", mode).apply {
        tlsPin?.let { put("tlsPin", it) }
        secure?.let { put("secure", it.toJson()) }
    }
    override fun toString() = "RelayProfile($name)"
    companion object {
        fun parse(text: String): RelayProfile {
            require(text.length <= 8192)
            val data = JSONObject(text)
            val version = data.getInt("version")
            require(version == 1 || version == 2)
            require(version != 1 || !data.has("secure")) // Never silently discard E2EE metadata.
            val raw = data.getString("url")
            require(raw.startsWith("wss://"))
            val url = raw.replaceFirst("wss://", "https://").toHttpUrl()
            require(url.username.isEmpty() && url.password.isEmpty() && url.query == null && url.fragment == null && url.encodedPath == "/")
            val device = data.getString("device")
            val token = data.getString("token")
            require(Regex("[a-zA-Z0-9_-]{1,64}").matches(device))
            require(Regex("[a-zA-Z0-9_-]{43}").matches(token))
            val name = data.optString("name", device).trim().take(80)
            require(name.isNotEmpty())
            val pin = data.optString("tlsPin").takeIf { it.isNotEmpty() }
            require(pin == null || Regex("sha256/[A-Za-z0-9+/]{43}=").matches(pin))
            val mode = data.optString("mode", "relay")
            require(mode in setOf("lan", "relay"))
            require(mode != "lan" || pin != null)
            val secure = if (version == 2) SecureRelayProfile.parse(data.getJSONObject("secure")) else null
            require(version != 2 || (pin != null && mode == "relay"))
            return RelayProfile(url.toString().replaceFirst("https://", "wss://").trimEnd('/'), device, token, name, pin, mode, secure)
        }
    }
}
