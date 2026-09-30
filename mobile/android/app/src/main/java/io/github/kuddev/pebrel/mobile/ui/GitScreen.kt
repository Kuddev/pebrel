package io.github.kuddev.pebrel.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.material3.TabRowDefaults.tabIndicatorOffset
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.git.*
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import java.text.DateFormat
import java.util.Date

@Composable
fun GitScreen(target: DesktopGitTarget, repository: SessionRepository, onBack: () -> Unit) {
    val controller = remember(target) { repository.desktopGit(target) }
    val state by controller.state.collectAsStateWithLifecycle()
    val desktops by repository.desktops.collectAsStateWithLifecycle()
    val drafts by repository.drafts.collectAsStateWithLifecycle()
    val desktop = desktops.find { it.id == target.desktop }
    val unavailable = target.availability(desktop, false) ?: controller.availability(false)
    LaunchedEffect(target, unavailable) {
        if (unavailable != null) {
            val pane = desktop?.panes?.find { it.window == target.window && it.id == target.pane }
            android.util.Log.w("PebrelGit", "availability=$unavailable generation=${target.generation}/${desktop?.connectionGeneration} process=${target.process}/${desktop?.runtimeProcess} pane=${pane != null} cwdEqual=${pane?.cwd == target.cwd} ssh=${pane?.sshDestination != null}")
        }
    }
    LaunchedEffect(controller) { controller.refresh() }
    GitScreenContent(target, desktop?.host?.name.orEmpty(), state, unavailable, desktop?.allowInput == true,
        drafts[target.draftKey].orEmpty(), { repository.setDraft(target.draftKey, it) }, controller, onBack)
}

@Composable
internal fun GitScreenContent(target: DesktopGitTarget, desktopName: String, state: GitViewState,
                              unavailable: String?, allowInput: Boolean, draft: String,
                              onDraft: (String) -> Unit, controller: DesktopGit, onBack: () -> Unit) {
    val canRead = unavailable == null && !state.busy
    val canWrite = canRead && allowInput && !state.needsRefresh
    var history by rememberSaveable(target) { mutableStateOf(false) }
    var details by remember(target) { mutableStateOf(false) }
    var collapsed by rememberSaveable(target, state.snapshot?.root) { mutableStateOf(emptyList<String>()) }
    // 列表位置归页面所有，打开 diff 或切换历史不应把用户送回文件树顶部。
    val changesScroll = rememberLazyListState()
    val historyScroll = rememberLazyListState()
    BackHandler(state.diff != null) { controller.closeDiff() }
    val back = { if (state.diff != null) controller.closeDiff() else onBack() }
    val root = state.snapshot?.root ?: target.cwd
    val diff = state.diff
    Column(Modifier.fillMaxSize()) {
        GitHeader(stringResource(if (diff == null) R.string.git_title else R.string.git_diff),
            if (diff == null) gitPathName(root) else diff.path.ifEmpty { diff.commit.take(12) }, back,
            onRefresh = {
                state.diff?.let(controller::openDiff) ?: if (history) controller.history() else controller.refresh()
            }, canRefresh = canRead, onDetails = { details = true })
        if (diff == null) {
            TabRow(selectedTabIndex = if (history) 1 else 0,
                containerColor = MaterialTheme.colorScheme.surface,
                contentColor = MaterialTheme.colorScheme.onSurface,
                indicator = { positions ->
                    TabRowDefaults.SecondaryIndicator(Modifier.tabIndicatorOffset(positions[if (history) 1 else 0]),
                        height = 2.dp, color = MaterialTheme.colorScheme.onSurface)
                }) {
                Tab(!history, { history = false }, modifier = Modifier.heightIn(min = 48.dp),
                    text = { Text(stringResource(R.string.git_changes), fontSize = 14.sp) })
                Tab(history, { history = true; controller.history() }, enabled = canRead,
                    modifier = Modifier.heightIn(min = 48.dp),
                    text = { Text(stringResource(R.string.git_history), fontSize = 14.sp) })
            }
        }
        if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth().height(2.dp)) else Spacer(Modifier.height(2.dp))
        if (diff == null) state.snapshot?.let { GitBranchSummary(it) }
        GitFeedback(state, unavailable, !allowInput)
        if (diff != null) {
            GitDiffContent(diff, state, canWrite, controller)
        } else if (history) {
            GitHistoryContent(state, controller, unavailable == null, historyScroll)
        } else {
            GitChangesContent(state.snapshot, state.busy, canRead, canWrite, draft, onDraft, controller,
                changesScroll, collapsed, { key -> collapsed = if (key in collapsed) collapsed - key else collapsed + key })
        }
    }
    if (details) AlertDialog(onDismissRequest = { details = false },
        title = { Text(stringResource(R.string.git_repository_details)) },
        text = { SelectionContainer {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(desktopName)
                Column {
                    HelperText(stringResource(R.string.terminal_directory))
                    Text(root, fontFamily = LocalTerminalFont.current)
                }
                state.snapshot?.let { snapshot ->
                    Column {
                        Text(snapshot.branch)
                        Text(snapshot.upstream ?: stringResource(R.string.git_no_upstream))
                        HelperText(stringResource(R.string.git_ahead_behind, snapshot.ahead, snapshot.behind))
                    }
                }
            }
        } }, confirmButton = { TextButton({ details = false }) { Text(stringResource(R.string.close)) } })
}

private fun gitPathName(path: String): String = path.replace('\\', '/').trimEnd('/').substringAfterLast('/').ifBlank { path }

@Composable
private fun GitHeader(title: String, subtitle: String, onBack: () -> Unit, onRefresh: () -> Unit,
                      canRefresh: Boolean, onDetails: () -> Unit) {
    Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).heightIn(min = 58.dp)
        .padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
        Column(Modifier.weight(1f).padding(horizontal = 4.dp, vertical = 8.dp)) {
            Text(title, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.SemiBold)
            Text(subtitle, fontSize = 12.sp, lineHeight = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        GlyphButton(R.drawable.ic_git_refresh, stringResource(R.string.git_refresh), onRefresh, canRefresh)
        GlyphButton(R.drawable.ic_info, stringResource(R.string.git_repository_details), onDetails)
    }
}

@Composable
private fun GitBranchSummary(snapshot: GitSnapshot) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(10.dp)
    val syncDescription = stringResource(R.string.git_ahead_behind, snapshot.ahead, snapshot.behind)
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)
        .background(colors.surface, shape).border(0.5.dp, colors.outlineVariant, shape).padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Glyph(R.drawable.ic_git_branch, Modifier.size(16.dp))
            Text(snapshot.branch, Modifier.weight(1f), fontSize = 14.sp, lineHeight = 20.sp,
                fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(if (snapshot.upstream == null) stringResource(R.string.git_no_upstream)
                else if (snapshot.ahead == 0 && snapshot.behind == 0) stringResource(R.string.git_up_to_date)
                else stringResource(R.string.git_sync_counts, snapshot.ahead, snapshot.behind),
                modifier = Modifier.semantics { if (snapshot.upstream != null) contentDescription = syncDescription },
                color = colors.onSurfaceVariant, fontSize = 12.sp, lineHeight = 18.sp)
        }
        Text(stringResource(R.string.git_change_counts, snapshot.entries.count(GitEntry::unstaged), snapshot.entries.count(GitEntry::staged)),
            Modifier.padding(top = 6.dp), fontSize = 12.sp, lineHeight = 18.sp, color = colors.onSurfaceVariant)
    }
}

@Composable
private fun GitFeedback(state: GitViewState, unavailable: String?, readonly: Boolean) {
    var details by remember { mutableStateOf(false) }
    val problem = unavailable?.let { GitProblem(it) } ?: state.error
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics { liveRegion = LiveRegionMode.Polite }) {
        if (unavailable == null && readonly) Text(stringResource(R.string.git_readonly), fontSize = 13.sp,
            modifier = Modifier.padding(bottom = 8.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (state.completed.isNotEmpty()) {
            val labels = state.completed.map { gitActionLabel(it) }.joinToString(" · ")
            Text(stringResource(R.string.git_completed, labels), fontSize = 13.sp, modifier = Modifier.padding(bottom = 8.dp))
        }
        if (problem != null) {
            Text(gitProblemText(problem), color = MaterialTheme.colorScheme.error, fontSize = 13.sp,
                modifier = Modifier.padding(vertical = 8.dp))
            if (problem.detail.isNotBlank()) TextButton({ details = true }) { Text(stringResource(R.string.git_error_details)) }
        }
    }
    if (details && problem != null) AlertDialog(onDismissRequest = { details = false },
        title = { Text(stringResource(R.string.git_error_details)) },
        text = { SelectionContainer { Text(problem.detail, Modifier.verticalScroll(rememberScrollState()), fontFamily = LocalTerminalFont.current) } },
        confirmButton = { TextButton({ details = false }) { Text(stringResource(R.string.close)) } })
}

@Composable
private fun GitChangesContent(snapshot: GitSnapshot?, busy: Boolean, canRead: Boolean, canWrite: Boolean, draft: String,
                              onDraft: (String) -> Unit, controller: DesktopGit, scroll: LazyListState,
                              collapsed: List<String>, onFolder: (String) -> Unit) {
    val staged = snapshot?.entries.orEmpty().filter(GitEntry::staged)
    val unstaged = snapshot?.entries.orEmpty().filter(GitEntry::unstaged)
    val tooLong = draft.toByteArray(Charsets.UTF_8).size > 16 * 1024
    var menu by remember { mutableStateOf(false) }
    val groups = listOf(
        Triple("work:", R.string.git_section_changes, unstaged.filter { it.worktree != "?" }),
        Triple("new:", R.string.git_section_untracked, unstaged.filter { it.worktree == "?" }),
        Triple("index:", R.string.git_section_staged, staged),
    )
    Column(Modifier.fillMaxSize()) {
        if (snapshot != null) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                GitBulkButton(R.drawable.ic_plus, stringResource(R.string.git_stage_all), canWrite && unstaged.isNotEmpty(), Modifier.weight(1f)) {
                    controller.mutate(GitAction.Stage, all = true)
                }
                GitBulkButton(R.drawable.ic_git_unstage, stringResource(R.string.git_unstage_all), canWrite && staged.isNotEmpty(), Modifier.weight(1f)) {
                    controller.mutate(GitAction.Unstage, all = true)
                }
                Box {
                    GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { menu = true }, !busy)
                    DropdownMenu(menu, { menu = false }) {
                        listOf(GitAction.Fetch, GitAction.Pull, GitAction.Push, GitAction.Sync).forEach { action ->
                            DropdownMenuItem(text = { Text(gitActionLabel(action)) }, enabled = canWrite,
                                onClick = { menu = false; controller.mutate(action) })
                        }
                    }
                }
            }
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = scroll, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp)) {
            if (snapshot?.entries?.isEmpty() == true) item("clean") {
                Column(Modifier.fillMaxWidth().padding(vertical = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Glyph(R.drawable.ic_check, Modifier.size(24.dp))
                    Text(stringResource(R.string.git_clean), Modifier.padding(top = 12.dp), fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            groups.forEach { (prefix, title, entries) ->
                if (entries.isNotEmpty()) {
                    val index = prefix == "index:"
                    val rows = gitTree(entries, collapsed.filter { it.startsWith(prefix) }.map { it.removePrefix(prefix) }.toSet())
                    item("${prefix}header") {
                        Row(Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(title), Modifier.weight(1f), fontSize = 12.sp, lineHeight = 18.sp,
                                fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(entries.size.toString(), fontSize = 12.sp, lineHeight = 18.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    items(rows, key = { prefix + it.path }) { row ->
                        val key = prefix + row.path
                        GitTreeItem(row, index, key in collapsed, canRead, canWrite,
                            onFolder = { onFolder(key) },
                            onDiff = { controller.openDiff(GitDiffSelection(row.path, index)) },
                            onStage = { controller.mutate(if (index) GitAction.Unstage else GitAction.Stage, path = row.path) })
                    }
                }
            }
        }
        if (snapshot != null) GitCommitBar(draft, onDraft, !busy, tooLong, staged.size,
            canWrite && staged.isNotEmpty() && snapshot.entries.none { it.conflict } && draft.isNotBlank() && !tooLong) {
            controller.mutate(GitAction.Commit, message = draft)
        }
    }
}

@Composable
private fun GitBulkButton(icon: Int, label: String, enabled: Boolean, modifier: Modifier, onClick: () -> Unit) {
    FilledTonalButton(onClick, modifier.heightIn(min = 48.dp), enabled = enabled, shape = RoundedCornerShape(8.dp),
        colors = ButtonDefaults.filledTonalButtonColors(containerColor = MaterialTheme.colorScheme.surfaceVariant,
            contentColor = MaterialTheme.colorScheme.onSurface), contentPadding = PaddingValues(horizontal = 8.dp, vertical = 8.dp)) {
        Glyph(icon, Modifier.size(14.dp), LocalContentColor.current)
        Spacer(Modifier.width(5.dp))
        Text(label, fontSize = 12.sp, lineHeight = 18.sp)
    }
}

@Composable
private fun GitCommitBar(draft: String, onDraft: (String) -> Unit, editable: Boolean, tooLong: Boolean,
                         stagedCount: Int, canCommit: Boolean, onCommit: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    var focused by remember { mutableStateOf(false) }
    val label = stringResource(R.string.git_commit_message)
    val commitLabel = stringResource(R.string.git_commit_staged, stagedCount)
    val shape = RoundedCornerShape(8.dp)
    Column(Modifier.fillMaxWidth().background(colors.surface)) {
        HorizontalDivider(color = colors.outlineVariant)
        // 底栏占真实布局空间；系统 IME inset 由 MainActivity 统一处理，避免盖住文件或重复抬升。
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            BasicTextField(draft, onDraft, Modifier.weight(1f).heightIn(min = 48.dp)
                .onFocusChanged { focused = it.isFocused }.semantics { contentDescription = label }
                .background(colors.background, shape)
                .border(if (focused) 1.dp else 0.5.dp, if (tooLong) colors.error else if (focused) colors.primary else colors.outline, shape)
                .padding(horizontal = 12.dp, vertical = 13.dp),
                enabled = editable, maxLines = 3, textStyle = TextStyle(color = colors.onSurface, fontSize = 14.sp, lineHeight = 20.sp),
                cursorBrush = SolidColor(colors.primary),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                decorationBox = { inner ->
                    if (draft.isEmpty()) Text(label, color = colors.onSurfaceVariant, fontSize = 14.sp, lineHeight = 20.sp)
                    inner()
                })
            Button(onCommit, Modifier.heightIn(min = 48.dp).semantics { contentDescription = commitLabel },
                enabled = canCommit, shape = shape,
                colors = ButtonDefaults.buttonColors(containerColor = colors.onSurface, contentColor = colors.background,
                    disabledContainerColor = colors.surfaceVariant, disabledContentColor = colors.onSurfaceVariant),
                contentPadding = PaddingValues(horizontal = 18.dp, vertical = 12.dp)) {
                Text(stringResource(R.string.git_commit), fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        if (tooLong) Text(stringResource(R.string.git_message_too_long), Modifier.padding(start = 16.dp, end = 16.dp, bottom = 8.dp),
            color = colors.error, fontSize = 12.sp)
    }
}

@Composable
private fun GitTreeItem(row: GitTreeRow, staged: Boolean, collapsed: Boolean, enabled: Boolean, canWrite: Boolean,
                        onFolder: () -> Unit, onDiff: () -> Unit, onStage: () -> Unit) {
    val entry = row.entry
    val colors = MaterialTheme.colorScheme
    val folderState = stringResource(if (collapsed) R.string.git_folder_collapsed else R.string.git_folder_expanded)
    val code = if (staged) entry?.index else entry?.worktree
    Column {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f).heightIn(min = 48.dp).clickable(enabled = enabled, role = Role.Button,
                onClick = if (entry == null) onFolder else onDiff)
                .padding(start = (row.depth.coerceAtMost(6) * 12).dp, top = 8.dp, bottom = 8.dp)
                .semantics { contentDescription = row.path; if (entry == null) stateDescription = folderState },
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.width(20.dp), contentAlignment = Alignment.Center) {
                    if (entry == null) Glyph(if (collapsed) R.drawable.ic_chevron else R.drawable.ic_down, Modifier.size(14.dp))
                    else Text(if (code == "?") "U" else code.orEmpty(), fontFamily = LocalTerminalFont.current,
                        fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                        color = if (entry.conflict || code == "D") colors.error else if (code == "A" || code == "?") colors.tertiary else colors.primary)
                }
                FileSymbol(row.path, entry == null, Modifier.size(20.dp), expanded = !collapsed)
                Column(Modifier.weight(1f)) {
                    Text(row.path.substringAfterLast('/'), fontSize = 14.sp, lineHeight = 20.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    if (entry?.original != null) Text(entry.original, fontSize = 12.sp, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    if (entry?.conflict == true) Text(stringResource(R.string.git_conflict), color = colors.error, fontSize = 12.sp)
                }
                if (entry != null) Glyph(R.drawable.ic_chevron, Modifier.size(14.dp))
            }
            if (entry != null) GlyphButton(if (staged) R.drawable.ic_git_unstage else R.drawable.ic_plus,
                stringResource(if (staged) R.string.git_unstage_file else R.string.git_stage_file, row.path), onStage, canWrite)
        }
        HorizontalDivider(color = colors.outlineVariant.copy(alpha = .6f), thickness = 0.5.dp)
    }
}

@Composable
private fun GitHistoryContent(state: GitViewState, controller: DesktopGit, connected: Boolean, scroll: LazyListState) {
    val dates = remember { DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT) }
    LazyColumn(Modifier.fillMaxSize(), state = scroll, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)) {
        if (!state.busy && state.history.isEmpty()) item { Text(stringResource(R.string.git_no_commits), Modifier.padding(vertical = 24.dp)) }
        items(state.history, key = GitCommit::hash) { commit ->
            Column(Modifier.fillMaxWidth().clickable(enabled = connected && !state.busy) { controller.openDiff(GitDiffSelection(commit = commit.hash)) }
                .padding(vertical = 12.dp)) {
                Text(commit.subject, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 3, overflow = TextOverflow.Ellipsis)
                Text("${commit.shortHash} · ${commit.author} · ${dates.format(Date(commit.timestamp * 1000))}",
                    fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 6.dp))
                if (commit.refs.isNotBlank()) Text(commit.refs, fontSize = 12.sp, color = MaterialTheme.colorScheme.primary)
            }
            HorizontalDivider()
        }
        if (state.hasMore) item { TextButton({ controller.history(more = true) }, enabled = connected && !state.busy) { Text(stringResource(R.string.git_load_more)) } }
    }
}

@Composable
private fun GitDiffContent(selection: GitDiffSelection, state: GitViewState, canWrite: Boolean, controller: DesktopGit) {
    val entry = state.snapshot?.entries?.find { it.path == selection.path }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(selection.path.ifEmpty { selection.commit.take(12) }, Modifier.weight(1f), fontFamily = LocalTerminalFont.current, fontSize = 13.sp)
            if (selection.commit.isEmpty() && entry != null) TextButton({
                controller.mutate(if (selection.staged) GitAction.Unstage else GitAction.Stage, path = selection.path)
            }, enabled = canWrite) { Text(gitActionLabel(if (selection.staged) GitAction.Unstage else GitAction.Stage)) }
        }
        if (selection.commit.isNotEmpty()) HelperText(stringResource(R.string.git_history_readonly), Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
        val lines = remember(state.diffText) { state.diffText?.split('\n').orEmpty() }
        if (!state.busy && state.diffText?.isBlank() == true) HelperText(stringResource(R.string.git_empty_diff), Modifier.padding(16.dp))
        SelectionContainer {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 20.dp)) {
                items(lines.size, key = { it }) { index ->
                    val line = lines[index]
                    val color = when {
                        line.startsWith("+") -> MaterialTheme.colorScheme.primary
                        line.startsWith("-") -> MaterialTheme.colorScheme.error
                        else -> MaterialTheme.colorScheme.onSurface
                    }
                    Text(line.ifEmpty { " " }, Modifier.fillMaxWidth().background(color.copy(alpha = if (line.startsWith('+') || line.startsWith('-')) .08f else 0f))
                        .padding(horizontal = 12.dp, vertical = 1.dp), fontFamily = LocalTerminalFont.current, fontSize = 13.sp, lineHeight = 19.sp, color = color)
                }
            }
        }
    }
}

@Composable
private fun gitActionLabel(action: GitAction): String = stringResource(when (action) {
    GitAction.Stage -> R.string.git_stage
    GitAction.Unstage -> R.string.git_unstage
    GitAction.Commit -> R.string.git_commit
    GitAction.Fetch -> R.string.git_fetch
    GitAction.Pull -> R.string.git_pull
    GitAction.Push -> R.string.git_push
    GitAction.Sync -> R.string.git_sync
})

@Composable
private fun gitProblemText(problem: GitProblem): String = stringResource(when (problem.code) {
    "method_not_found" -> R.string.git_update_desktop
    "git_disconnected", "runtime_connection_lost" -> R.string.git_disconnected
    "git_target_changed", "target_not_found" -> R.string.git_target_changed
    "input_not_authorized" -> R.string.git_readonly
    "git_stale" -> R.string.git_stale
    "git_busy" -> R.string.git_busy
    "git_not_committable" -> R.string.git_not_committable
    "git_too_large", "git_encoding" -> R.string.git_large_output
    "git_refresh_failed" -> R.string.git_refresh_failed
    "git_outcome_unknown", "runtime_timeout" -> R.string.git_unknown_result
    "remote_exec_unsupported", "exec_context_unavailable" -> R.string.git_host_only
    else -> R.string.git_failed
})
