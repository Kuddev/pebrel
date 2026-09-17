package io.github.kuddev.pebrel.mobile.connection

/** One authority for separate fields and the familiar user@host address form. */
data class SshEndpoint(val address: String, val user: String)

fun parseSshEndpoint(address: String, user: String): SshEndpoint {
    val input = address.trim()
    require(input.isNotEmpty() && !input.contains("://") && !input.any { it.isWhitespace() || it.isISOControl() })
    val separator = input.indexOf('@')
    require(separator == input.lastIndexOf('@'))
    val login = (if (separator >= 0) input.substring(0, separator) else user).trim()
    val host = if (separator >= 0) input.substring(separator + 1) else input
    require(login.isNotEmpty() && login.none { it.isWhitespace() || it.isISOControl() || it == '@' })
    require(host.isNotEmpty() && host.none { it in "/?#" })
    val unwrapped = if (host.startsWith('[') && host.endsWith(']')) host.substring(1, host.length - 1) else host
    require(unwrapped.isNotEmpty() && unwrapped.none { it == '[' || it == ']' })
    return SshEndpoint(unwrapped, login)
}

val HostProfile.endpointLabel: String
    get() {
        val endpoint = runCatching { parseSshEndpoint(address, user) }.getOrNull() ?: return "$address:$port"
        val host = if (endpoint.address.contains(':')) "[${endpoint.address}]" else endpoint.address
        return "${endpoint.user}@$host:$port"
    }
