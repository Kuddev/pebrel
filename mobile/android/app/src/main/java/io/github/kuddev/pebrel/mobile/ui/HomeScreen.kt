package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.ExperimentalAnimationApi
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.HostProfile
import io.github.kuddev.pebrel.mobile.connection.endpointLabel
import io.github.kuddev.pebrel.mobile.connection.RelayProfile
import io.github.kuddev.pebrel.mobile.connection.DesktopPane
import io.github.kuddev.pebrel.mobile.connection.DesktopTab
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.LocalSession

@Composable
fun HomeHeader(onSettings: () -> Unit, onNotices: () -> Unit, showNotices: Boolean = true) {
    Row(Modifier.fillMaxWidth().height(67.dp).background(MaterialTheme.colorScheme.surface).padding(horizontal = 17.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Image(painterResource(R.drawable.ic_pebrel), null, Modifier.size(24.dp))
        Text("Pebrel", fontSize = 19.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f).padding(start = 9.dp))
        if (showNotices) GlyphButton(R.drawable.ic_bell, stringResource(R.string.notifications), onNotices)
        GlyphButton(R.drawable.ic_settings, stringResource(R.string.settings), onSettings)
    }
}

@OptIn(ExperimentalAnimationApi::class)
@Composable
fun HomeScreen(
    sessions: List<LocalSession>, hosts: List<HostProfile>, desktops: List<DesktopWorkspace>, relays: List<RelayProfile>,
    onSession: (String) -> Unit, onSessions: () -> Unit, onHosts: () -> Unit, onLogin: (HostProfile) -> Unit,
    onEditHost: (HostProfile) -> Unit, onDeleteHost: (HostProfile) -> Unit, onAddHost: () -> Unit,
    onDesktop: (String) -> Unit, onRelay: (RelayProfile) -> Unit, onComputers: () -> Unit,
    onAddRelay: () -> Unit, onLocal: () -> Unit, onDeployRelay: () -> Unit,
    onPane: ((String, DesktopPane) -> Unit)? = null,
    onTab: ((String, DesktopTab) -> Unit)? = null,
) {
    if (isHomeEmpty(sessions, hosts, desktops, relays)) {
        EmptyHomeScreen(onAddRelay, onAddHost, onLocal)
        return
    }
    val motion = rememberPebrelMotion()
    val cards = sessionCards(sessions, desktops)
    var newConnection by rememberSaveable { mutableStateOf(false) }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val thumbnailWidth = ((maxWidth - 44.dp) * .45f).coerceAtLeast(133.dp)
        // 按 fontScale 留高；大字号 sp 的非线性换算会让卡片高度不随小标签一起放大。
        val thumbnailHeight = 180.dp * LocalDensity.current.fontScale.coerceAtLeast(1f)
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 22.dp, end = 22.dp, top = 8.dp, bottom = 105.dp)) {
            item {
                GroupHeading(stringResource(R.string.sessions), cards.size, stringResource(R.string.all_sessions), onSessions)
                Spacer(Modifier.height(8.dp))
                AnimatedContent(
                    targetState = cards,
                    contentKey = { visibleSessions -> visibleSessions.map { it.key } },
                    transitionSpec = { motion.collectionTransition() },
                    label = "home_sessions",
                ) { visibleSessions ->
                    if (visibleSessions.isEmpty()) {
                        Row(Modifier.fillMaxWidth().heightIn(min = 96.dp).animateContentSize(motion.contentSizeSpec())
                            .workspaceFrame().padding(16.dp), verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                            WorkspaceSymbol { Glyph(R.drawable.ic_terminal, Modifier.size(22.dp)) }
                            HelperText(stringResource(R.string.no_sessions))
                        }
                    } else LazyRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                        items(visibleSessions, key = { it.key }) { card ->
                            val local = card.local
                            val thumbnailModifier = Modifier.width(thumbnailWidth).height(thumbnailHeight)
                            if (local != null) SessionThumbnail(local, thumbnailModifier) { onSession(local.id) }
                            else DesktopSessionThumbnail(checkNotNull(card.desktop), card.pane, thumbnailModifier, card.tab) {
                                if (card.tab != null && onTab != null) onTab.invoke(card.desktop.id, card.tab)
                                else if (card.pane != null && onPane != null) onPane.invoke(card.desktop.id, card.pane)
                                else onDesktop(card.desktop.id)
                            }
                        }
                    }
                }
            }
            item {
                Spacer(Modifier.height(22.dp))
                GroupHeading(stringResource(R.string.ssh_hosts), action = stringResource(R.string.all_count, hosts.size), onAction = onHosts)
                Spacer(Modifier.height(8.dp))
            }
            item {
                AnimatedContent(
                    targetState = hosts.take(2),
                    contentKey = { visibleHosts -> visibleHosts.map { it.id } },
                    transitionSpec = { motion.collectionTransition() },
                    label = "home_hosts",
                ) { visibleHosts ->
                    Column(Modifier.animateContentSize(motion.contentSizeSpec())) {
                        if (visibleHosts.isEmpty()) {
                            HelperText(stringResource(R.string.no_hosts), Modifier.fillMaxWidth().workspaceFrame().padding(18.dp))
                        } else {
                            visibleHosts.forEach { host ->
                                HostRow(host, { onLogin(host) }, { onEditHost(host) }, { onDeleteHost(host) })
                            }
                        }
                    }
                }
            }
            item {
                Spacer(Modifier.height(26.dp))
                GroupHeading(stringResource(R.string.computers), action = stringResource(R.string.all), onAction = onComputers)
                ComputerRows(desktops, relays, onDesktop, onRelay, onAddRelay)
                Spacer(Modifier.height(22.dp))
                NavigationRow(R.drawable.ic_terminal, stringResource(R.string.local_terminal), onClick = onLocal)
            }
        }
        FloatingActionButton({ newConnection = true }, shape = CircleShape, containerColor = MaterialTheme.colorScheme.primary,
            contentColor = MaterialTheme.colorScheme.onPrimary,
            modifier = Modifier.align(Alignment.BottomEnd).padding(end = 22.dp, bottom = 23.dp).size(58.dp)) {
            Icon(painterResource(R.drawable.ic_plus), stringResource(R.string.new_connection), Modifier.size(27.dp))
        }
    }
    if (newConnection) NewConnectionSheet(
        onDismiss = { newConnection = false },
        onComputer = onAddRelay,
        onSshHost = onAddHost,
        onLocal = onLocal,
        onDeployRelay = onDeployRelay,
    )
}

internal fun isHomeEmpty(sessions: List<LocalSession>, hosts: List<HostProfile>,
                         desktops: List<DesktopWorkspace>, relays: List<RelayProfile>): Boolean =
    // 暂时没有会话不代表首次使用；已保存的主机和电脑必须仍可打开。
    sessions.isEmpty() && hosts.isEmpty() && relays.isEmpty() && desktops.none { it.hasConnected }

internal data class SessionCard(val key: String, val local: LocalSession? = null,
                                val desktop: DesktopWorkspace? = null, val pane: DesktopPane? = null, val tab: DesktopTab? = null)

internal fun sessionCards(sessions: List<LocalSession>, desktops: List<DesktopWorkspace>): List<SessionCard> = buildList {
    sessions.forEach { add(SessionCard("local:${it.id}", local = it)) }
    desktops.filter { it.hasConnected }.forEach { computer ->
        if (computer.tabs.isNotEmpty()) computer.tabs.forEach { tab ->
            add(SessionCard("pc:${computer.id}:${tab.key}", desktop = computer, pane = tab.primaryPane, tab = tab))
        }
        else if (computer.panes.isEmpty()) add(SessionCard("pc:${computer.id}", desktop = computer))
        else computer.panes.forEach { pane ->
            add(SessionCard("pc:${computer.id}:${pane.window}:${pane.id}", desktop = computer, pane = pane))
        }
    }
}

@Composable
private fun DesktopSessionThumbnail(desktop: DesktopWorkspace, pane: DesktopPane?, modifier: Modifier, tab: DesktopTab? = null, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val status = statusLabel(desktop.status)
    Column(modifier.workspaceFrame().clickable(onClick = onClick)) {
        Column(Modifier.weight(1f).fillMaxWidth().background(colors.surfaceVariant.copy(alpha = .3f)).padding(10.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Glyph(tab?.let(::tabIcon) ?: R.drawable.ic_monitor, Modifier.size(14.dp))
                Text("${desktop.transport} · $status", fontSize = 12.sp, lineHeight = 18.sp, color = colors.onSurfaceVariant,
                    modifier = Modifier.weight(1f).padding(start = 6.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(10.dp))
            Text(desktop.host.name, fontSize = 14.sp, lineHeight = 20.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            pane?.task?.takeIf(String::isNotBlank)?.let {
                Text(it, fontSize = 12.sp, lineHeight = 18.sp, fontFamily = LocalTerminalFont.current, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(tab?.file?.path ?: pane?.cwd.orEmpty(), fontSize = 12.sp, lineHeight = 18.sp, fontFamily = LocalTerminalFont.current, maxLines = 3,
                color = colors.onSurfaceVariant, overflow = TextOverflow.Ellipsis)
        }
        Text(tab?.displayTitle ?: pane?.displayTitle ?: desktop.host.name, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, maxLines = 1,
            overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth().padding(horizontal = 11.dp, vertical = 9.dp))
    }
}

@Composable
private fun SessionThumbnail(session: LocalSession, modifier: Modifier, onClick: () -> Unit) {
    // One bounded capture when the gallery appears. No hidden terminal renderer or polling per card.
    val preview = remember(session.id, session.status) {
        session.terminal.previewText()
    }
    val colors = MaterialTheme.colorScheme
    val source = if (session.source == "Local") stringResource(R.string.local_device) else session.source
    val status = statusLabel(session.status)
    Column(modifier.workspaceFrame().clickable(onClick = onClick)) {
        Column(Modifier.weight(1f).fillMaxWidth().background(colors.surfaceVariant.copy(alpha = .3f)).padding(10.dp)) {
            Row(Modifier.fillMaxWidth().heightIn(min = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(8.dp).semantics { stateDescription = status }, contentAlignment = Alignment.Center) {
                    if (session.status == "connecting") CircularProgressIndicator(Modifier.size(8.dp), strokeWidth = 1.2.dp)
                    else Box(Modifier.size(6.dp).background(when (session.status) {
                        "ready", "finished" -> colors.tertiary
                        "failed" -> colors.error
                        else -> colors.onSurfaceVariant.copy(alpha = .6f)
                    }, CircleShape))
                }
                Spacer(Modifier.width(8.dp))
                Text(source, fontSize = 12.sp, lineHeight = 18.sp, color = colors.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).wrapContentWidth(Alignment.End)
                        .background(colors.background.copy(alpha = .65f), RoundedCornerShape(50))
                        .padding(horizontal = 7.dp, vertical = 3.dp))
            }
            Spacer(Modifier.height(5.dp))
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (preview.isBlank()) Glyph(R.drawable.ic_terminal, Modifier.align(Alignment.Center).size(22.dp))
                else Text(preview, fontSize = 9.sp, lineHeight = 13.sp, fontFamily = LocalTerminalFont.current,
                    color = colors.onSurfaceVariant, softWrap = false, maxLines = 6, overflow = TextOverflow.Clip)
            }
        }
        Text(session.title, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, maxLines = 1,
            overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth().padding(horizontal = 11.dp, vertical = 9.dp))
    }
}

@Composable
fun HostRow(host: HostProfile, onLogin: () -> Unit, onEdit: () -> Unit, onDelete: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(bottom = 10.dp).workspaceFrame().heightIn(min = 83.dp).padding(start = 13.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f).clickable(onClick = onLogin).padding(vertical = 15.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            WorkspaceSymbol { HostSymbol(host.icon, Modifier.size(23.dp)) }
            Column(Modifier.weight(1f)) {
                Text(host.name, fontSize = 16.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(host.endpointLabel, fontFamily = LocalTerminalFont.current, fontSize = 12.sp, lineHeight = 18.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 5.dp))
            }
        }
        Box {
            GlyphButton(R.drawable.ic_more, "${stringResource(R.string.host_actions)} · ${host.name}", { menu = true })
            DropdownMenu(menu, { menu = false }) {
                DropdownMenuItem(text = { Text(stringResource(R.string.edit_host)) }, onClick = { menu = false; onEdit() })
                DropdownMenuItem(text = { Text(stringResource(R.string.delete_host)) }, onClick = { menu = false; onDelete() })
            }
        }
    }
}

@Composable
fun ComputerRows(desktops: List<DesktopWorkspace>, relays: List<RelayProfile>, onDesktop: (String) -> Unit,
                 onRelay: (RelayProfile) -> Unit, onAddRelay: () -> Unit, onForget: ((RelayProfile) -> Unit)? = null) {
    var removing by remember { mutableStateOf<RelayProfile?>(null) }
    val computers = computerSummaries(desktops, relays)
    val motion = rememberPebrelMotion()
    Column(Modifier.fillMaxWidth().animateContentSize(motion.contentSizeSpec())) {
        if (computers.isEmpty()) HelperText(stringResource(R.string.no_computers), Modifier.padding(bottom = 8.dp))
        Column(Modifier.fillMaxWidth().workspaceFrame()) {
            computers.forEach { computer ->
                key(computer.id) {
                    val relay = computer.relay
                    val profileId = desktops.find { it.id == computer.id }?.host?.id ?: computer.id
                    val saved = relays.find { it.id == profileId }
                    ComputerRow(computer.title, computer.status, computer.transport, computer.address, computer.icon,
                        onForget = if (saved != null && onForget != null) ({ removing = saved }) else null) {
                        if (relay == null) onDesktop(computer.id) else onRelay(relay)
                    }
                    HorizontalDivider(Modifier.padding(start = 56.dp), color = MaterialTheme.colorScheme.outlineVariant)
                }
            }
            ComputerRow(stringResource(R.string.relay_connect), onClick = onAddRelay)
        }
    }
    removing?.let { profile ->
        AlertDialog(onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.computer_remove_title)) },
            text = { Text(stringResource(R.string.computer_remove_hint, profile.name)) },
            confirmButton = { TextButton({ removing = null; onForget?.invoke(profile) }) {
                Text(stringResource(R.string.computer_remove_title))
            } },
            dismissButton = { TextButton({ removing = null }) { Text(stringResource(R.string.cancel)) } })
    }
}

private data class ComputerSummary(
    val id: String,
    val title: String,
    val status: String,
    val transport: String,
    val address: String,
    val relay: RelayProfile? = null,
    val icon: String = "term",
)

private fun computerSummaries(desktops: List<DesktopWorkspace>, relays: List<RelayProfile>): List<ComputerSummary> = buildList {
    // A failed first attempt lives only on its connection page, not in the
    // user's computer library. Existing, previously connected PCs remain.
    desktops.filter { it.hasConnected || relays.any { profile -> profile.id == it.host.id } }.forEach { desktop ->
        val retryProfile = if (desktop.status in setOf("ready", "connecting", "approval")) null
            else relays.find { it.id == desktop.host.id }
        add(ComputerSummary(desktop.id, desktop.host.name, desktop.status, desktop.transport, desktop.host.address, retryProfile, desktop.host.icon))
    }
    relays.filter { profile -> desktops.none { it.host.id == profile.id } }.forEach { profile ->
        add(ComputerSummary(profile.id, profile.name, "disconnected", if (profile.mode == "lan") "LAN" else "Relay", profile.url, profile, profile.hostOs))
    }
}

@Composable
private fun ComputerRow(title: String, status: String? = null, transport: String = "", address: String = "", icon: String = "term",
                        onForget: (() -> Unit)? = null, onClick: () -> Unit) {
    // 电脑和添加入口属于同一级；共用图标列与行高，点击留白也有完整的按压反馈。
    Row(Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick)
        .heightIn(min = 72.dp).padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        if (status == null) Glyph(R.drawable.ic_plus, Modifier.size(24.dp)) else HostSymbol(icon, Modifier.size(24.dp))
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (status != null) Box(Modifier.padding(top = 4.dp)) {
                val endpoint = address.removePrefix("wss://").trimEnd('/')
                StatusCaption(status, listOf(transport, endpoint).filter(String::isNotBlank).joinToString(" · ", postfix = " · "))
            }
        }
        Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
            if (onForget != null) {
                var menu by remember { mutableStateOf(false) }
                GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { menu = true })
                DropdownMenu(menu, { menu = false }) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.computer_remove_title)) },
                        onClick = { menu = false; onForget() })
                }
            } else Glyph(R.drawable.ic_chevron, Modifier.size(14.dp))
        }
    }
}

@Composable
fun HostsScreen(hosts: List<HostProfile>, onLogin: (HostProfile) -> Unit, onEdit: (HostProfile) -> Unit, onDelete: (HostProfile) -> Unit) {
    var query by remember { mutableStateOf("") }
    var group by remember { mutableStateOf("all") }
    val found = hosts.filter { (group == "all" || it.group == group) && "${it.name} ${it.address} ${it.user}".contains(query, ignoreCase = true) }
    LazyColumn(contentPadding = PaddingValues(horizontal = 22.dp, vertical = 12.dp)) {
        item {
            ConnectionSearchField(query, { query = it }, Modifier.fillMaxWidth().padding(bottom = 10.dp))
        }
        item {
            ConnectionSegments(listOf("all" to stringResource(R.string.all), "production" to stringResource(R.string.group_production),
                "development" to stringResource(R.string.group_development)), group, { group = it }, Modifier.fillMaxWidth().padding(bottom = 14.dp))
        }
        items(found, key = { it.id }) { host -> HostRow(host, { onLogin(host) }, { onEdit(host) }, { onDelete(host) }) }
        item { HelperText(stringResource(if (found.isEmpty() && query.isNotBlank()) R.string.no_search_results else R.string.host_hint), Modifier.padding(top = 20.dp)) }
    }
}
