package io.github.kuddev.pebrel.mobile.ui

import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.*
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

@Composable
fun SftpFileScreen(session: LocalSession, tab: SftpTab, onBack: () -> Unit, onTabs: () -> Unit,
                   onOpen: (SftpEntry) -> Unit, onClose: () -> Unit) {
    val client = checkNotNull(session.files)
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var content by remember(tab.id) { mutableStateOf<SftpContent?>(null) }
    var failure by remember(tab.id) { mutableStateOf<String?>(null) }
    var notice by remember(tab.id) { mutableStateOf<Int?>(null) }
    var loading by remember(tab.id) { mutableStateOf(false) }
    var refresh by remember(tab.id) { mutableIntStateOf(0) }
    var preview by rememberSaveable(tab.id) { mutableStateOf(true) }
    var menu by remember { mutableStateOf(false) }
    var details by remember { mutableStateOf(false) }
    var outline by remember { mutableStateOf(false) }
    var heading by remember { mutableStateOf<String?>(null) }
    var imageReset by remember { mutableIntStateOf(0) }
    var download by remember(tab.id) { mutableStateOf<Job?>(null) }
    var cancelling by remember { mutableStateOf(false) }
    var transferred by remember { mutableLongStateOf(0) }
    var transferTotal by remember { mutableStateOf<Long?>(null) }
    val ready = session.status == "ready"
    val file = content?.file ?: tab.file
    val image = isSftpImage(file.path)
    val markdown = file.path.substringAfterLast('.').lowercase() in setOf("md", "markdown")
    val clipboard = rememberReaderClipboard(file.name) { failure = "clipboard_failed" }
    val copyLabel = stringResource(R.string.reader_copy)
    val copiedLabel = stringResource(R.string.reader_copied)
    val imageLabel = stringResource(R.string.reader_open_image)
    val colors = MaterialTheme.colorScheme.readerColors()
    LaunchedEffect(client, tab.id, session.status, refresh) {
        if (!ready) return@LaunchedEffect
        loading = true; failure = null
        try { content = client.preview(tab.file.path) }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = sftpFailureCode(error) }
        finally { loading = false }
    }
    LaunchedEffect(heading) { if (heading != null) { delay(200); heading = null } }
    val text by produceState<String?>(null, content, image) {
        value = null
        val bytes = content?.bytes
        if (bytes != null && !image) {
            try { value = withContext(Dispatchers.Default) {
                if (bytes.any { it == 0.toByte() }) throw NativeSshException("SFTP_BINARY")
                Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString()
            } }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { failure = "SFTP_BINARY" }
        }
    }
    val document by produceState<ReaderDocument?>(null, text, preview, colors, copyLabel, imageLabel) {
        value = text?.let { source -> withContext(Dispatchers.Default) { prepareReader(source, file.path, markdown && preview, colors, copyLabel, imageLabel) } }
    }
    val bitmap by produceState<ImageBitmap?>(null, content, image) {
        value = null
        val bytes = content?.bytes
        if (bytes != null && image) {
            try { value = withContext(Dispatchers.IO) { decodeReaderImage(bytes) } }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { failure = "image_decode_failed" }
        }
    }
    val saveDocument = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) {
            transferred = 0; transferTotal = file.size; cancelling = false; failure = null; notice = null
            download = scope.launch {
                var complete = false
                try {
                    withContext(Dispatchers.IO) {
                        checkNotNull(context.contentResolver.openOutputStream(uri, "w")).use { output ->
                            client.download(file.path, output) { count, total -> transferred = count; transferTotal = total }
                        }
                    }
                    complete = true; notice = R.string.sftp_downloaded
                } catch (cancelled: CancellationException) { notice = R.string.sftp_cancelled; throw cancelled }
                catch (error: Exception) { failure = sftpFailureCode(error) }
                finally {
                    // 只清理这次 CreateDocument 新建的未完成文件，不触碰其他本地文件。
                    if (!complete) withContext(NonCancellable + Dispatchers.IO) {
                        runCatching { DocumentsContract.deleteDocument(context.contentResolver, uri) }
                    }
                    download = null; cancelling = false
                }
            }
        }
    }
    fun openLink(link: String) {
        if (link.startsWith('#')) { heading = link; return }
        val uri = Uri.parse(link)
        if (uri.scheme in setOf("https", "http", "mailto")) {
            try { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
            catch (_: Exception) { failure = "link_open_failed" }
            return
        }
        val path = resolveSftpLink(file.path, link) ?: run { failure = "link_open_failed"; return }
        scope.launch {
            try {
                val entry = client.stat(path)
                if (entry.directory) failure = "SFTP_FILE_TYPE" else onOpen(entry)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = sftpFailureCode(error) }
        }
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
            Row(Modifier.weight(1f).heightIn(min = 48.dp).clickable(onClick = onTabs), verticalAlignment = Alignment.CenterVertically) {
                Glyph(if (image) R.drawable.ic_image else R.drawable.ic_git_file, Modifier.size(18.dp))
                Text(file.name, Modifier.weight(1f).padding(horizontal = 8.dp), maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 14.sp)
                Glyph(R.drawable.ic_down, Modifier.size(12.dp))
            }
            Box {
                GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { menu = true })
                DropdownMenu(menu, { menu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.reader_refresh)) }, { menu = false; refresh++ }, enabled = ready && !loading)
                    DropdownMenuItem({ Text(stringResource(R.string.sftp_download)) }, { menu = false; saveDocument.launch(file.name) }, enabled = ready && download == null)
                    if (markdown && preview) DropdownMenuItem({ Text(stringResource(R.string.reader_outline)) }, { menu = false; outline = true }, enabled = !document?.headings.isNullOrEmpty())
                    if (text != null) DropdownMenuItem({ Text(stringResource(R.string.reader_copy_source)) }, { menu = false; text?.let { clipboard.copy(it, -1) } })
                    if (image) DropdownMenuItem({ Text(stringResource(R.string.reader_reset_zoom)) }, { menu = false; imageReset++ })
                    DropdownMenuItem({ Text(stringResource(R.string.reader_details)) }, { menu = false; details = true })
                    DropdownMenuItem({ Text(stringResource(R.string.tab_close)) }, { menu = false; onClose() })
                }
            }
        }
        if (markdown) TabRow(if (preview) 0 else 1, containerColor = MaterialTheme.colorScheme.background) {
            Tab(preview, { preview = true }, text = { Text(stringResource(R.string.reader_preview)) })
            Tab(!preview, { preview = false }, text = { Text(stringResource(R.string.reader_source)) })
        }
        if (!ready) HelperText(stringResource(R.string.sftp_disconnected), Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        if (clipboard.copied == -1) HelperText(copiedLabel, Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        notice?.let { HelperText(stringResource(it), Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) }
        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (download != null) SftpTransferProgress(transferred, transferTotal, cancelling) { cancelling = true; download?.cancel() }
        failure?.let { code -> SftpFailureRow(code, if (ready && !loading) ({ refresh++ }) else null) }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            val prepared = document
            val loadedImage = bitmap
            when {
                prepared != null -> ReaderWebView(prepared, colors, clipboard.copied, copyLabel, copiedLabel, heading,
                    { clipboard.copy(prepared.code[it], it) }, ::openLink, Modifier.fillMaxSize())
                loadedImage != null -> ReaderImage(loadedImage, file.name, imageReset)
                !loading && failure != null -> TextButton({ saveDocument.launch(file.name) }, Modifier.align(Alignment.Center), enabled = ready && download == null) {
                    Text(stringResource(R.string.sftp_download))
                }
            }
        }
    }
    if (details) AlertDialog(onDismissRequest = { details = false }, title = { Text(file.name) },
        text = { androidx.compose.foundation.text.selection.SelectionContainer { Text("${session.host?.name.orEmpty()}\n${file.path}") } },
        confirmButton = { TextButton({ details = false }) { Text(stringResource(R.string.close)) } })
    if (outline) AlertDialog(onDismissRequest = { outline = false }, title = { Text(stringResource(R.string.reader_outline)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState())) {
            document?.headings?.forEach { item -> TextButton({ heading = item.id; outline = false }, Modifier.fillMaxWidth().padding(start = ((item.level - 1) * 10).dp)) {
                Text(item.text, Modifier.fillMaxWidth())
            } }
        } }, confirmButton = { TextButton({ outline = false }) { Text(stringResource(R.string.close)) } })
}
