package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.DesktopFailureKind

@Composable
fun desktopFailureText(kind: DesktopFailureKind): String = stringResource(when (kind) {
    DesktopFailureKind.DNS -> R.string.desktop_dns
    DesktopFailureKind.TIMEOUT -> R.string.desktop_timeout
    DesktopFailureKind.REFUSED -> R.string.desktop_refused
    DesktopFailureKind.NETWORK -> R.string.desktop_network
    DesktopFailureKind.CERTIFICATE_CHANGED -> R.string.desktop_certificate_changed
    DesktopFailureKind.CERTIFICATE_DATE -> R.string.desktop_certificate_date
    DesktopFailureKind.TLS -> R.string.desktop_tls
    DesktopFailureKind.AUTHENTICATION -> R.string.desktop_authentication
    DesktopFailureKind.ALREADY_CONNECTED -> R.string.desktop_already_connected
    DesktopFailureKind.SERVER -> R.string.desktop_server
    DesktopFailureKind.PEER_OFFLINE -> R.string.desktop_peer_offline
    DesktopFailureKind.RUNTIME_UNAVAILABLE -> R.string.desktop_runtime_unavailable
    DesktopFailureKind.PROTOCOL -> R.string.desktop_protocol
    DesktopFailureKind.DISCONNECTED -> R.string.desktop_disconnected
    DesktopFailureKind.UNKNOWN -> R.string.desktop_connection_failed
})
