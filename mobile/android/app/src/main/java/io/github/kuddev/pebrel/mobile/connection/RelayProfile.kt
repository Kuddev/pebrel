package io.github.kuddev.pebrel.mobile.connection

import okhttp3.HttpUrl.Companion.toHttpUrl
import org.json.JSONObject
import java.security.MessageDigest

/** Persisted invitations share the same wire contract for LAN and relayed connections. */
class RelayProfile(val url: String, val device: String, val token: String, val name: String,
                   val tlsPin: String? = null, val mode: String = "relay", secure: SecureRelayProfile? = null) {
    @Volatile var secure: SecureRelayProfile? = secure
        internal set
    @Volatile var hostOs: String = "term"
        internal set
    val version: Int = if (secure == null) 1 else 2
    internal val legacyId: String = digest("$url/$device" + (tlsPin?.let { "/$it" } ?: "") + (secure?.let { "/${it.host}" } ?: ""))
    // 地址、房间和授权可以轮换；电脑身份只跟随 Noise 公钥，认证成功前不写入保存记录。
    val id: String = secure?.let { digest("pebrel.mobile.host.v2\n${it.host}") } ?: legacyId

    internal fun sameConnection(other: RelayProfile): Boolean {
        val first = secure
        val second = other.secure
        return url == other.url && device == other.device && token == other.token && tlsPin == other.tlsPin && mode == other.mode &&
            first?.host == second?.host && first?.grant == second?.grant && first?.secret == second?.secret &&
            first?.invitation == second?.invitation
    }
    internal val canRediscover: Boolean get() = mode == "lan" && tlsPin != null && secure?.invitation == false

    internal fun discoveredAt(computer: PairingComputer): RelayProfile? {
        // mDNS 不是授权来源：只移动已配对且 SPKI 匹配的端点，绝不采纳广播中的新凭据。
        if (!canRediscover || computer.pin != tlsPin) return null
        return runCatching { atLanAddress(computer.address.toString()) }.getOrNull()?.takeIf { it.url != url }
    }

    internal fun atLanAddress(value: String): RelayProfile {
        require(canRediscover)
        val raw = value.trim()
        require(raw.length in 1..512 && raw.none(Char::isWhitespace))
        val address = when {
            raw.startsWith("wss://") -> raw.replaceFirst("wss://", "https://")
            "://" !in raw -> "https://$raw"
            else -> raw
        }.toHttpUrl()
        require(address.scheme == "https" && address.username.isEmpty() && address.password.isEmpty() &&
            address.encodedPath == "/" && address.query == null && address.fragment == null)
        val nextUrl = address.toString().replaceFirst("https://", "wss://").trimEnd('/')
        return RelayProfile(nextUrl, device, token, name, tlsPin, mode, secure).also { it.hostOs = hostOs }
    }
    fun toJson(): JSONObject = JSONObject().put("version", version).put("url", url).put("device", device).put("token", token).put("name", name).put("mode", mode).apply {
        put("hostOs", hostOs)
        tlsPin?.let { put("tlsPin", it) }
        secure?.let { put("secure", it.toJson()) }
    }
    override fun toString() = "RelayProfile($name)"
    companion object {
        private fun digest(value: String): String = MessageDigest.getInstance("SHA-256")
            .digest(value.toByteArray(Charsets.UTF_8)).joinToString("") { "%02x".format(it) }

        // 保存列表按成功连接时间追加；旧版留下的同机记录应保留最后一次有效连接。
        internal fun latestByComputer(profiles: List<RelayProfile>): List<RelayProfile> =
            profiles.asReversed().distinctBy { it.id }.asReversed()

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
            require(version != 2 || pin != null)
            return RelayProfile(url.toString().replaceFirst("https://", "wss://").trimEnd('/'), device, token, name, pin, mode, secure).also {
                it.hostOs = desktopOsIcon(data.optString("hostOs"))
            }
        }
    }
}
