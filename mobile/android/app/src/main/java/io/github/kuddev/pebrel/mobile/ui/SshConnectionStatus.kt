package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.remember
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.SshFailureKind
import io.github.kuddev.pebrel.mobile.connection.SshStage
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.TrustRequest

@Composable
fun SshConnectionStatus(session: LocalSession, onCancel: () -> Unit, onRetry: () -> Unit, onEdit: () -> Unit,
                        trust: TrustRequest? = null, onTrust: (Boolean) -> Unit = {}) {
    BackHandler(onBack = onCancel)
    val connecting = session.status == "connecting"
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background.copy(alpha = .22f))
        .clickable(remember { MutableInteractionSource() }, indication = null) {}, contentAlignment = Alignment.Center) {
        ElevatedCard(Modifier.padding(24.dp).widthIn(max = 420.dp).fillMaxWidth(),
            colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.elevatedCardElevation(8.dp)) {
            Column(Modifier.padding(20.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (connecting && trust == null) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                    else Glyph(if (trust != null) R.drawable.ic_server else R.drawable.ic_info, Modifier.size(23.dp))
                    Column(Modifier.weight(1f)) {
                        Text(session.title, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                        Text(stringResource(if (trust != null) R.string.verify_host else if (connecting) R.string.establishing_connection else R.string.connection_failed),
                            fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                if (trust != null) {
                    HelperText("${trust.host.address}:${trust.host.port}")
                    HelperText(stringResource(R.string.verify_hint))
                    SelectionContainer {
                        Text(trust.fingerprint, fontSize = 12.sp, lineHeight = 20.sp, fontFamily = LocalTerminalFont.current)
                    }
                    ConnectionButton(stringResource(R.string.trust_connect)) { onTrust(true) }
                    TextButton({ onTrust(false) }) { Text(stringResource(R.string.cancel)) }
                } else {
                    Text(stringResource(if (connecting) stageText(session.stage) else failureText(session.failure)),
                        fontSize = 12.sp, lineHeight = 20.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
                    if (connecting) TextButton(onCancel) { Text(stringResource(R.string.cancel)) }
                    else {
                        ConnectionButton(stringResource(R.string.retry), onClick = onRetry)
                        TextButton(onEdit) { Text(stringResource(R.string.back_edit)) }
                    }
                }
            }
        }
    }
}

@Composable
fun TerminalDisconnected(session: LocalSession, onRetry: (() -> Unit)?) {
    Surface(color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .45f)) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp)) {
            HelperText(stringResource(if (session.status == "ended") R.string.session_ended_hint else failureText(session.failure)))
            onRetry?.let { retry -> TextButton(retry) { Text(stringResource(R.string.retry)) } }
        }
    }
}

private fun stageText(stage: SshStage): Int = when (stage) {
    SshStage.NETWORK -> R.string.ssh_stage_network
    SshStage.VERIFYING -> R.string.ssh_stage_verifying
    SshStage.AUTHENTICATING -> R.string.ssh_stage_auth
    SshStage.OPENING_SHELL -> R.string.ssh_stage_shell
}

private fun failureText(kind: SshFailureKind?): Int = when (kind) {
    SshFailureKind.UNKNOWN_HOST -> R.string.ssh_error_dns
    SshFailureKind.TIMEOUT -> R.string.ssh_error_timeout
    SshFailureKind.REFUSED -> R.string.ssh_error_refused
    SshFailureKind.AUTH -> R.string.ssh_error_auth
    SshFailureKind.HOST_KEY_CHANGED -> R.string.ssh_error_host_key
    SshFailureKind.TRUST_REJECTED -> R.string.ssh_error_trust
    SshFailureKind.CHANNEL -> R.string.ssh_error_channel
    SshFailureKind.NETWORK -> R.string.ssh_error_network
    SshFailureKind.CRYPTO -> R.string.ssh_error_crypto
    SshFailureKind.NEGOTIATION -> R.string.ssh_error_negotiation
    else -> R.string.ssh_error_unknown
}
