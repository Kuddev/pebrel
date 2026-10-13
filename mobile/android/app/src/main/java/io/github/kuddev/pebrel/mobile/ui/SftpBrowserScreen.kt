package io.github.kuddev.pebrel.mobile.ui

import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*

@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
fun SftpBrowserScreen(session: LocalSession, initialPath: String, onPath: (String) -> Unit,
                      onBack: () -> Unit, onFile: (SftpEntry) -> Unit) {
    val client = checkNotNull(session.files)
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var requested by rememberSaveable(session.id) { mutableStateOf(initialPath) }
    var location by rememberSaveable(session.id) { mutableStateOf(initialPath) }
    var rows by remember(session.id) { mutableStateOf(emptyList<SftpEntry>()) }
    var cursor by remember(session.id) { mutableStateOf<Long?>(null) }
    var loading by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<Int?>(null) }
    var refresh by remember { mutableIntStateOf(0) }
    var showHidden by rememberSaveable { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    var addMenu by remember { mutableStateOf(false) }
    var naming by remember { mutableStateOf<String?>(null) }
    var selected by remember { mutableStateOf<SftpEntry?>(null) }
    var actionMenu by remember { mutableStateOf<String?>(null) }
    var selectedPaths by remember(session.id) { mutableStateOf(setOf<String>()) }
    var selectionAnchor by remember(session.id) { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf<SftpEntry?>(null) }
    var actionBusy by remember { mutableStateOf(false) }
    var actionError by remember { mutableStateOf<String?>(null) }
    var uploading by remember { mutableStateOf<Job?>(null) }
    var cancellingUpload by remember { mutableStateOf(false) }
    var paging by remember { mutableStateOf<Job?>(null) }
    var listingGeneration by remember { mutableIntStateOf(0) }
    var uploadUri by remember { mutableStateOf<Uri?>(null) }
    var uploadName by remember { mutableStateOf("") }
    var uploadSize by remember { mutableStateOf<Long?>(null) }
    var transferred by remember { mutableLongStateOf(0L) }
    var transferTotal by remember { mutableStateOf<Long?>(null) }
    val ready = session.status == "ready"
    val listState = rememberLazyListState()
    val pickUpload = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                val metadata = withContext(Dispatchers.IO) {
                    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { result ->
                        if (result.moveToFirst()) result.getString(0) to (if (result.isNull(1)) null else result.getLong(1).takeIf { it >= 0 }) else null
                    }
                }
                uploadUri = uri
                uploadName = metadata?.first?.takeIf { runCatching { sftpChild(location, it) }.isSuccess } ?: "upload.bin"
                uploadSize = metadata?.second
                naming = "upload"
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { failure = "SFTP_LOCAL_FILE" }
        }
    }
    fun clearSelection() { selectedPaths = emptySet(); selectionAnchor = null }
    fun navigate(path: String) {
        actionMenu = null
        refreshing = false
        clearSelection()
        notice = null
        if (path == location) { refresh++; return }
        requested = path
    }
    fun back() {
        if (selectedPaths.isNotEmpty()) clearSelection()
        // 页面返回与目录导航分开：访问目录不压页面栈，父目录由「..」负责。
        else onBack()
    }
    BackHandler { back() }
    LaunchedEffect(naming) { actionError = null; if (naming != null) notice = null }
    LaunchedEffect(requested, refresh, session.status) {
        val generation = ++listingGeneration
        paging?.cancelAndJoin(); paging = null
        val previousCursor = cursor
        cursor = null
        // 刷新和换目录前释放旧分页游标，避免反复下拉耗尽同一 SSH 连接的目录句柄。
        if (previousCursor != null) withContext(NonCancellable) { runCatching { client.closeList(previousCursor) } }
        if (!ready) { loading = false; refreshing = false; return@LaunchedEffect }
        loading = true; failure = null
        try {
            val result = client.list(requested)
            ensureActive()
            rows = result.entries; cursor = result.cursor; location = result.path
            clearSelection()
            onPath(result.path)
            if (result.skipped > 0) notice = R.string.sftp_skipped
            // 新目录与滚动位置在下一次测量一同生效，不强制重测尚未替换的旧条目。
            listState.requestScrollToItem(0)
        } catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = sftpFailureCode(error) }
        finally { if (generation == listingGeneration) { loading = false; refreshing = false } }
    }
    LaunchedEffect(client) {
        try { awaitCancellation() }
        finally {
            withContext(NonCancellable) {
                paging?.cancelAndJoin()
                uploading?.cancelAndJoin()
                cursor?.let { id -> runCatching { client.closeList(id) } }
            }
        }
    }
    fun open(entry: SftpEntry) {
        if (entry.directory) navigate(entry.path)
        else if (entry.kind == "symlink") scope.launch {
            try { val resolved = client.stat(entry.path); if (resolved.directory) navigate(resolved.path) else onFile(resolved) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = sftpFailureCode(error) }
        } else onFile(entry)
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            GlyphButton(R.drawable.ic_back, stringResource(R.string.back), ::back)
            Column(Modifier.weight(1f).clip(MaterialTheme.shapes.medium).clickable { clearSelection(); naming = "path" }
                .padding(horizontal = 8.dp, vertical = 6.dp)) {
                Text(if (selectedPaths.isEmpty()) location.trimEnd('/').substringAfterLast('/').ifBlank { "/" }
                    else stringResource(R.string.sftp_selected_count, selectedPaths.size),
                    maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 14.sp)
                Text("SFTP · ${session.host?.name ?: session.title}", color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 11.sp, maxLines = 1)
            }
            if (selectedPaths.isNotEmpty()) TextButton(::clearSelection) {
                Text(stringResource(R.string.cancel))
            }
            Box {
                GlyphButton(R.drawable.ic_plus, stringResource(R.string.sftp_add), { clearSelection(); addMenu = true }, ready && uploading == null)
                DropdownMenu(addMenu, { addMenu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.sftp_upload)) }, { addMenu = false; pickUpload.launch(arrayOf("*/*")) })
                    DropdownMenuItem({ Text(stringResource(R.string.sftp_new_folder)) }, { addMenu = false; naming = "mkdir" })
                }
            }
            Box {
                GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { clearSelection(); menu = true })
                DropdownMenu(menu, { menu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.sftp_refresh)) }, { menu = false; notice = null; refresh++ }, enabled = ready && !loading)
                    DropdownMenuItem({ Text(stringResource(R.string.sftp_go_path)) }, { menu = false; naming = "path" })
                    DropdownMenuItem({ Text(stringResource(if (showHidden) R.string.sftp_hide_hidden else R.string.sftp_show_hidden)) }, {
                        menu = false; showHidden = !showHidden; clearSelection()
                    })
                    DropdownMenuItem({ Text(stringResource(R.string.chat_terminal)) }, { menu = false; onBack() })
                }
            }
        }
        if (!ready) HelperText(stringResource(R.string.sftp_disconnected), Modifier.padding(16.dp))
        if (loading && !refreshing) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (uploading != null) SftpTransferProgress(transferred, transferTotal, cancellingUpload) {
            cancellingUpload = true; uploading?.cancel()
        }
        notice?.let { HelperText(stringResource(it), Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) }
        failure?.let { SftpFailureRow(it, if (ready && !loading) ({ notice = null; refresh++ }) else null) }
        val visible = remember(rows, showHidden) {
            rows.filter { showHidden || !it.name.startsWith('.') }.sortedWith(compareBy<SftpEntry> { !it.directory }.thenBy { it.name.lowercase() })
        }
        val visiblePaths = remember(visible) { visible.map { it.path } }
        PullToRefreshBox(isRefreshing = refreshing, onRefresh = {
            if (ready && !loading) {
                clearSelection(); actionMenu = null; notice = null; refreshing = true; refresh++
            }
        }, modifier = Modifier.weight(1f).fillMaxWidth()) {
            LazyColumn(Modifier.fillMaxSize().testTag("sftp-file-list"), state = listState, contentPadding = PaddingValues(vertical = 4.dp)) {
                // 父目录只有导航语义，独立于 selectable entries，范围选择不会包含它。
                if (location != "/" && location != ".") item {
                    SftpParentRow(ready && !loading) { navigate(sftpParent(location)) }
                }
                items(visible, key = { it.path }) { entry ->
                    val selectedEntry = entry.path in selectedPaths
                    SftpEntryRow(entry, selectedEntry, ready && !loading, actionMenu == entry.path,
                        onMenu = { expanded ->
                            if (expanded) clearSelection()
                            actionMenu = if (expanded) entry.path else null
                        },
                        onOpen = {
                            if (selectedPaths.isEmpty()) open(entry)
                            else {
                                selectedPaths = if (selectedEntry) selectedPaths - entry.path else selectedPaths + entry.path
                                if (selectedPaths.isEmpty()) selectionAnchor = null
                            }
                        },
                        onSwipe = {
                            selectedPaths = selectedPaths + sftpSelectionRange(visiblePaths, selectionAnchor, entry.path)
                            selectionAnchor = entry.path
                        },
                        onRename = { selected = entry; naming = "rename" },
                        onDelete = { deleting = entry })
                }
                if (visible.isEmpty() && !loading && failure == null) item {
                    HelperText(stringResource(R.string.sftp_empty), Modifier.padding(horizontal = 16.dp, vertical = 28.dp))
                }
                if (cursor != null) item {
                    TextButton({
                        if (loading) return@TextButton
                        loading = true; failure = null
                        val generation = listingGeneration
                        val path = location
                        val ownedCursor = cursor
                        paging = scope.launch {
                            try {
                                val result = client.list(path, ownedCursor)
                                if (generation != listingGeneration || result.path != location) return@launch
                                if (rows.size + result.entries.size > 8192) throw NativeSshException("SFTP_DIRECTORY_LIMIT")
                                rows = (rows + result.entries).distinctBy { it.path }; cursor = result.cursor
                            } catch (cancelled: CancellationException) { throw cancelled }
                            catch (error: Exception) {
                                failure = sftpFailureCode(error)
                                // 闲置游标已由 SSH 层回收，继续提交同一游标只会重复失败；保留列表供刷新恢复。
                                if (failure == "SFTP_STALE") cursor = null
                            }
                            finally { if (generation == listingGeneration) { loading = false; paging = null } }
                        }
                    }, Modifier.fillMaxWidth(), enabled = !loading && ready) { Text(stringResource(R.string.sftp_more)) }
                }
            }
        }
    }
    naming?.let { action ->
        val title = stringResource(when (action) { "path" -> R.string.sftp_go_path; "mkdir" -> R.string.sftp_new_folder; "rename" -> R.string.sftp_rename; else -> R.string.sftp_upload })
        SftpNameDialog(title, when (action) { "path" -> location; "rename" -> selected?.name.orEmpty(); "upload" -> uploadName; else -> "" }, actionBusy, actionError,
            { naming = null; if (action == "rename") selected = null }, { name ->
                if (action == "path") { naming = null; navigate(name); return@SftpNameDialog }
                val path = runCatching { sftpChild(location, name) }.getOrElse { actionError = "INVALID_INPUT"; return@SftpNameDialog }
                if (action == "upload") {
                    val uri = uploadUri ?: return@SftpNameDialog
                    naming = null; transferred = 0; transferTotal = uploadSize; notice = null; failure = null; cancellingUpload = false
                    uploading = scope.launch {
                        try {
                            withContext(Dispatchers.IO) {
                                checkNotNull(context.contentResolver.openInputStream(uri)).use { stream ->
                                    client.upload(path, stream, uploadSize) { count, total -> transferred = count; transferTotal = total }
                                }
                            }
                            notice = R.string.sftp_uploaded; refresh++
                        } catch (cancelled: CancellationException) { notice = R.string.sftp_cancelled; throw cancelled }
                        catch (error: Exception) { failure = sftpFailureCode(error) }
                        finally { uploading = null; uploadUri = null; cancellingUpload = false }
                    }
                } else {
                    actionBusy = true; actionError = null
                    scope.launch {
                        try {
                            if (action == "mkdir") client.mkdir(path) else client.rename(checkNotNull(selected), path)
                            naming = null; selected = null; refresh++
                        } catch (cancelled: CancellationException) { throw cancelled }
                        catch (error: Exception) { actionError = sftpFailureCode(error) }
                        finally { actionBusy = false }
                    }
                }
            })
    }
    deleting?.let { entry -> SftpDeleteDialog(entry, client, { deleting = null }) { deleting = null; refresh++ } }
}

@Composable
internal fun SftpNameDialog(title: String, initial: String, busy: Boolean, error: String?, onDismiss: () -> Unit, onSubmit: (String) -> Unit) {
    var name by remember(title, initial) { mutableStateOf(initial) }
    AlertDialog(onDismissRequest = { if (!busy) onDismiss() }, title = { Text(title) },
        text = { Column {
            OutlinedTextField(name, { name = it }, singleLine = true, enabled = !busy, label = { Text(title) }, isError = error != null,
                supportingText = error?.let { { Text(sftpFailureText(it)) } })
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        } },
        confirmButton = { TextButton({ onSubmit(name) }, enabled = name.isNotBlank() && !busy) { Text(stringResource(R.string.sftp_confirm)) } },
        dismissButton = { TextButton(onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel)) } })
}

@Composable
private fun SftpDeleteDialog(entry: SftpEntry, client: SftpClient, onDismiss: () -> Unit, onDeleted: () -> Unit) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(onDismissRequest = { if (!busy) onDismiss() }, title = { Text(stringResource(R.string.sftp_delete)) },
        text = { Column { Text(stringResource(R.string.sftp_delete_confirm, entry.name)); error?.let { Text(sftpFailureText(it), color = MaterialTheme.colorScheme.error) } } },
        confirmButton = { TextButton({
            busy = true
            scope.launch {
                try { client.remove(entry); onDeleted() }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (failure: Exception) { error = sftpFailureCode(failure) }
                finally { busy = false }
            }
        }, enabled = !busy) { Text(stringResource(R.string.sftp_delete), color = MaterialTheme.colorScheme.error) } },
        dismissButton = { TextButton(onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel)) } })
}
