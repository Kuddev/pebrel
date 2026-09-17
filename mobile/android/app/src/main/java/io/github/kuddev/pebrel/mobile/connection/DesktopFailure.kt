package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.TimeoutCancellationException
import org.json.JSONException
import java.io.IOException
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.security.cert.CertificateException
import java.security.cert.CertificateExpiredException
import java.security.cert.CertificateNotYetValidException
import javax.net.ssl.SSLException
import javax.net.ssl.SSLPeerUnverifiedException

/** Stable, bounded diagnostics. Remote text and invitation credentials never reach the UI. */
enum class DesktopFailureKind(val code: String) {
    DNS("desktop_dns"), TIMEOUT("desktop_timeout"), REFUSED("desktop_refused"),
    NETWORK("desktop_network"), CERTIFICATE_CHANGED("desktop_certificate_changed"),
    CERTIFICATE_DATE("desktop_certificate_date"), TLS("desktop_tls"),
    AUTHENTICATION("desktop_authentication"), ALREADY_CONNECTED("desktop_already_connected"),
    SERVER("desktop_server"), PEER_OFFLINE("desktop_peer_offline"),
    RUNTIME_UNAVAILABLE("desktop_runtime_unavailable"), PROTOCOL("desktop_protocol"),
    DISCONNECTED("desktop_disconnected"), UNKNOWN("desktop_connection_failed"),
}

class DesktopConnectionFailure(val kind: DesktopFailureKind, cause: Throwable? = null) : IOException(kind.code, cause)
internal class PairingCertificateChanged : CertificateException("Pairing certificate changed")

fun classifyDesktopFailure(error: Throwable?): DesktopFailureKind {
    val causes = generateSequence(error) { it.cause }.take(12).toList()
    causes.filterIsInstance<DesktopConnectionFailure>().firstOrNull()?.let { return it.kind }
    return when {
        causes.any { it is PairingCertificateChanged } -> DesktopFailureKind.CERTIFICATE_CHANGED
        causes.any { it is CertificateExpiredException || it is CertificateNotYetValidException } -> DesktopFailureKind.CERTIFICATE_DATE
        causes.any { it is SSLPeerUnverifiedException || it is SSLException || it is CertificateException } -> DesktopFailureKind.TLS
        causes.any { it is UnknownHostException } -> DesktopFailureKind.DNS
        causes.any { it is SocketTimeoutException || it is TimeoutCancellationException } -> DesktopFailureKind.TIMEOUT
        causes.any { it is ConnectException } -> DesktopFailureKind.REFUSED
        causes.any { it is NoRouteToHostException || it is IOException } -> DesktopFailureKind.NETWORK
        causes.any { it is JSONException } -> DesktopFailureKind.PROTOCOL
        else -> DesktopFailureKind.UNKNOWN
    }
}

internal fun desktopHttpFailure(status: Int?): DesktopFailureKind? = when (status) {
    null, 101 -> null
    401, 403 -> DesktopFailureKind.AUTHENTICATION
    409 -> DesktopFailureKind.ALREADY_CONNECTED
    else -> DesktopFailureKind.SERVER
}
