package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
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

@Composable
fun SshConnectionStatus(session: LocalSession, onCancel: () -> Unit, onRetry: () -> Unit, onEdit: () -> Unit) {
    BackHandler(onBack = onCancel)
    val connecting = session.status == "connecting"
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 10.dp)) {
        ConnectionHeading(stringResource(if (connecting) R.string.connect else R.string.connection_failed), onCancel)
        Column(Modifier.fillMaxWidth().padding(top = 40.dp, bottom = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Box(Modifier.size(64.dp).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .4f), MaterialTheme.shapes.large),
                contentAlignment = Alignment.Center) {
                if (connecting) CircularProgressIndicator(Modifier.size(25.dp), strokeWidth = 2.dp)
                else Glyph(R.drawable.ic_server, Modifier.size(28.dp), MaterialTheme.colorScheme.error)
            }
            Text(stringResource(if (connecting) R.string.establishing_connection else R.string.connection_failed),
                fontSize = 18.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(top = 22.dp, bottom = 12.dp))
            Text(session.title, fontSize = 14.sp)
            session.host?.let { host ->
                HelperText("${host.user}@${host.address}:${host.port}", Modifier.padding(top = 8.dp))
            }
            Text(stringResource(if (connecting) stageText(session.stage) else failureText(session.failure)),
                fontSize = 12.sp, lineHeight = 21.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(top = 24.dp).semantics { liveRegion = LiveRegionMode.Polite })
        }
        if (connecting) ConnectionButton(stringResource(R.string.cancel), primary = false, onClick = onCancel)
        else {
            ConnectionButton(stringResource(R.string.retry), onClick = onRetry)
            Spacer(Modifier.height(12.dp))
            ConnectionButton(stringResource(R.string.back_edit), primary = false, onClick = onEdit)
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
