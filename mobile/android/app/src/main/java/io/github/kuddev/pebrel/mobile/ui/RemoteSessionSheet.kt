package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.RemoteAttachment
import io.github.kuddev.pebrel.mobile.connection.RemoteSession
import io.github.kuddev.pebrel.mobile.connection.RemoteWindow
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.CancellationException

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteSessionSheet(local: LocalSession, repository: SessionRepository, onClose: () -> Unit,
                       onAttach: (RemoteAttachment) -> Unit) {
    var expanded by remember(local.id) { mutableStateOf<RemoteSession?>(null) }
    var windows by remember(expanded) { mutableStateOf<List<RemoteWindow>>(emptyList()) }
    var loading by remember(expanded) { mutableStateOf(false) }
    var failed by remember(expanded) { mutableStateOf(false) }
    LaunchedEffect(local.id, expanded, local.status) {
        val selected = expanded ?: return@LaunchedEffect
        if (local.status != "ready") return@LaunchedEffect
        loading = true
        try { windows = repository.remoteWindows(local.id, selected) }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (_: Exception) { failed = true }
        finally { loading = false }
    }
    ModalBottomSheet(onDismissRequest = onClose) {
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 620.dp), contentPadding = PaddingValues(20.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)) {
            item {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.remote_sessions), style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                    TextButton({ expanded = null; repository.refreshRemoteSessions(local.id) }, enabled = !local.discovering && local.status == "ready") {
                        Text(stringResource(R.string.refresh))
                    }
                }
                if (local.discovering) LinearProgressIndicator(Modifier.fillMaxWidth())
                if (local.discoveryFailed || local.remote?.warnings?.isNotEmpty() == true) HelperText(stringResource(R.string.remote_discovery_failed))
                if (!local.discovering && local.remote?.sessions.isNullOrEmpty()) HelperText(stringResource(R.string.remote_sessions_empty))
            }
            items(local.remote?.sessions.orEmpty(), key = { "${it.kind}:${it.id}" }) { session ->
                Card(Modifier.fillMaxWidth()) {
                    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(enabled = local.status == "ready") {
                        expanded = if (expanded?.id == session.id && expanded?.kind == session.kind) null else session
                    }.padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(session.name)
                            Text(session.kind, style = MaterialTheme.typography.labelSmall)
                        }
                        TextButton({ onAttach(RemoteAttachment(session)) }, enabled = local.status == "ready") {
                            Text(stringResource(R.string.connect))
                        }
                    }
                    if (expanded?.id == session.id && expanded?.kind == session.kind) Column(Modifier.fillMaxWidth().padding(horizontal = 14.dp)) {
                        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
                        if (failed) HelperText(stringResource(R.string.remote_discovery_failed))
                        windows.forEach { entry ->
                            Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(enabled = local.status == "ready") {
                                onAttach(RemoteAttachment(session, entry))
                            }.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                                Column(Modifier.weight(1f)) {
                                    Text(entry.label, style = MaterialTheme.typography.bodyMedium)
                                    if (entry.state.isNotBlank()) Text(entry.state, style = MaterialTheme.typography.labelSmall)
                                }
                                Glyph(R.drawable.ic_chevron, Modifier.size(16.dp))
                            }
                        }
                    }
                }
            }
            item { HelperText(stringResource(R.string.remote_sessions_hint)) }
        }
    }
}
