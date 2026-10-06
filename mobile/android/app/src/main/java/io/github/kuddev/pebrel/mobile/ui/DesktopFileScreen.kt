package io.github.kuddev.pebrel.mobile.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import kotlinx.coroutines.*

@Composable
fun DesktopFileScreen(desktop: DesktopWorkspace, tab: DesktopTab, repository: SessionRepository,
                      onBack: () -> Unit, onTabs: () -> Unit, onOpen: (DesktopTab) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val client = remember(desktop.id, desktop.connectionGeneration, desktop.runtimeProcess) { repository.desktopTabs(desktop.id) }
    var content by remember(tab.key, desktop.connectionGeneration) { mutableStateOf<DesktopFileContent?>(null) }
    var loadRevision by remember(tab.key) { mutableStateOf<Long?>(null) }
    var failure by remember(tab.key) { mutableStateOf<String?>(null) }
    var busy by remember(tab.key) { mutableStateOf(false) }
    var progress by remember(tab.key) { mutableFloatStateOf(0f) }
    var refresh by remember(tab.key) { mutableIntStateOf(0) }
    var preview by rememberSaveable(tab.key) { mutableStateOf(true) }
    var menu by remember { mutableStateOf(false) }
    var details by remember { mutableStateOf(false) }
    var outline by remember { mutableStateOf(false) }
    var closing by remember { mutableStateOf(false) }
    var heading by remember { mutableStateOf<String?>(null) }
    val clipboard = rememberReaderClipboard(tab.displayTitle) { failure = "clipboard_failed" }
    val copied = clipboard.copied
    var imageReset by remember { mutableIntStateOf(0) }
    val file = checkNotNull(tab.file)
    val markdown = file.path.substringAfterLast('.').lowercase() in setOf("md", "markdown")
    LaunchedEffect(tab.key, desktop.connectionGeneration, desktop.status, file.ready, refresh) {
        if (desktop.status != "ready" || !file.ready) return@LaunchedEffect
        busy = true; failure = null; progress = 0f
        try {
            val loaded = client.read(tab) { count, total -> progress = if (total == 0) 1f else count.toFloat() / total }
            ensureActive()
            content = loaded
            loadRevision = file.revision
        } catch (cancelled: CancellationException) { throw cancelled }
        catch (error: Exception) { failure = tabFailureCode(error) }
        finally { busy = false }
    }
    LaunchedEffect(heading) { if (heading != null) { delay(200); heading = null } }
    val colors = MaterialTheme.colorScheme.readerColors()
    val copyLabel = stringResource(R.string.reader_copy)
    val copiedLabel = stringResource(R.string.reader_copied)
    val imageLabel = stringResource(R.string.reader_open_image)
    val document by produceState<ReaderDocument?>(null, content, preview, colors) {
        val loaded = content
        value = if (loaded?.kind == "text") withContext(Dispatchers.Default) {
            prepareReader(loaded.bytes.toString(Charsets.UTF_8), file.path, markdown && preview, colors, copyLabel, imageLabel)
        } else null
    }
    val bitmap by produceState<ImageBitmap?>(null, content) {
        value = null
        val loaded = content
        if (loaded?.kind == "image") {
            try { value = withContext(Dispatchers.IO) { decodeReaderImage(loaded.bytes) } }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { failure = "image_decode_failed" }
        }
    }
    fun copy(text: String, index: Int) {
        clipboard.copy(text, index)
    }
    fun openLink(link: String) {
        if (link.startsWith('#')) { heading = link; return }
        val uri = Uri.parse(link)
        if (uri.scheme in setOf("https", "http", "mailto")) {
            try { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
            catch (_: Exception) { failure = "link_open_failed" }
            return
        }
        val path = resolveReaderLink(file, link)
        if (path == null) { failure = "link_open_failed"; return }
        if (busy) return
        scope.launch {
            busy = true
            try { onOpen(client.openFile(tab.window, path)) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) { failure = tabFailureCode(error) }
            finally { busy = false }
        }
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
            Row(Modifier.weight(1f).heightIn(min = 48.dp).clickable(onClick = onTabs), verticalAlignment = Alignment.CenterVertically) {
                DesktopTabSymbol(tab, Modifier.size(20.dp))
                Text(tab.displayTitle, Modifier.weight(1f).padding(horizontal = 8.dp), maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 14.sp)
                Glyph(R.drawable.ic_down, Modifier.size(12.dp))
            }
            GlyphButton(R.drawable.ic_git_refresh, stringResource(R.string.reader_refresh), { refresh++ }, !busy && desktop.status == "ready")
            Box {
                GlyphButton(R.drawable.ic_more, stringResource(R.string.more_actions), { menu = true })
                DropdownMenu(menu, { menu = false }) {
                    if (markdown && preview) DropdownMenuItem({ Text(stringResource(R.string.reader_outline)) }, { menu = false; outline = true }, enabled = !document?.headings.isNullOrEmpty())
                    if (content?.kind == "text") DropdownMenuItem({ Text(if (copied == -1) copiedLabel else stringResource(R.string.reader_copy_source)) }, {
                        menu = false; content?.let { copy(it.bytes.toString(Charsets.UTF_8), -1) }
                    })
                    if (content?.kind == "image") DropdownMenuItem({ Text(stringResource(R.string.reader_reset_zoom)) }, { menu = false; imageReset++ })
                    DropdownMenuItem({ Text(stringResource(R.string.reader_details)) }, { menu = false; details = true })
                    DropdownMenuItem({ Text(stringResource(R.string.tab_close)) }, { menu = false; closing = true }, enabled = desktop.allowInput && !busy)
                }
            }
        }
        if (markdown) TabRow(if (preview) 0 else 1, containerColor = MaterialTheme.colorScheme.background) {
            Tab(preview, { preview = true }, text = { Text(stringResource(R.string.reader_preview)) })
            Tab(!preview, { preview = false }, text = { Text(stringResource(R.string.reader_source)) })
        }
        val changed = content != null && file.revision != null && file.revision != loadRevision
        if (desktop.status != "ready" || file.dirty || changed || copied == -1) {
            HelperText(when {
                desktop.status != "ready" -> stringResource(R.string.reader_offline)
                copied == -1 -> copiedLabel
                changed -> stringResource(R.string.reader_changed)
                else -> stringResource(R.string.reader_unsaved)
            }, Modifier.padding(horizontal = 16.dp, vertical = 5.dp))
        }
        if (busy) LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
        failure?.let { code -> Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(tabFailureText(code), Modifier.weight(1f), color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
            TextButton({ refresh++ }) { Text(stringResource(R.string.retry)) }
        } }
        Box(Modifier.fillMaxWidth().weight(1f)) {
            val prepared = document
            val loadedImage = bitmap
            when {
                prepared != null -> ReaderWebView(prepared, colors, copied, copyLabel, copiedLabel, heading,
                    { index -> copy(prepared.code[index], index) }, ::openLink, Modifier.fillMaxSize())
                loadedImage != null -> ReaderImage(loadedImage, tab.displayTitle, imageReset)
                failure == null -> CircularProgressIndicator(Modifier.align(Alignment.Center))
            }
        }
    }
    if (details) AlertDialog(onDismissRequest = { details = false }, title = { Text(tab.displayTitle) },
        text = { androidx.compose.foundation.text.selection.SelectionContainer { Text("${desktop.host.name}\n${file.path}") } },
        confirmButton = { TextButton({ details = false }) { Text(stringResource(R.string.close)) } })
    if (outline) AlertDialog(onDismissRequest = { outline = false }, title = { Text(stringResource(R.string.reader_outline)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState())) {
            document?.headings?.forEach { item ->
                TextButton({ heading = item.id; outline = false }, Modifier.fillMaxWidth().padding(start = ((item.level - 1) * 10).dp)) {
                    Text(item.text, Modifier.fillMaxWidth())
                }
            }
        } }, confirmButton = { TextButton({ outline = false }) { Text(stringResource(R.string.close)) } })
    if (closing) DesktopTabCloseDialog(tab, client, { closing = false }, onBack)
}

@Composable
internal fun ReaderImage(bitmap: ImageBitmap, title: String, reset: Int) {
    var scale by remember(bitmap, reset) { mutableFloatStateOf(1f) }
    var offset by remember(bitmap, reset) { mutableStateOf(Offset.Zero) }
    BoxWithConstraints(Modifier.fillMaxSize().clipToBounds()) {
        val width = constraints.maxWidth.toFloat()
        val height = constraints.maxHeight.toFloat()
        val fit = minOf(width / bitmap.width, height / bitmap.height)
        Image(bitmap, title, Modifier.fillMaxSize()
            .pointerInput(bitmap, reset) { detectTapGestures(onDoubleTap = { scale = 1f; offset = Offset.Zero }) }
            .pointerInput(bitmap, reset, width, height) { detectTransformGestures { centroid, pan, zoom, _ ->
                val next = (scale * zoom).coerceIn(1f, 8f)
                val anchor = centroid - Offset(width / 2, height / 2)
                val moved = (offset - anchor) * (next / scale) + anchor + pan
                val maxX = ((bitmap.width * fit * next - width) / 2).coerceAtLeast(0f)
                val maxY = ((bitmap.height * fit * next - height) / 2).coerceAtLeast(0f)
                offset = Offset(moved.x.coerceIn(-maxX, maxX), moved.y.coerceIn(-maxY, maxY)); scale = next
            } }
            .graphicsLayer { scaleX = scale; scaleY = scale; translationX = offset.x; translationY = offset.y })
    }
}

internal fun decodeReaderImage(bytes: ByteArray): ImageBitmap {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
    check(options.outWidth > 0 && options.outHeight > 0)
    options.inSampleSize = 1
    while (options.outWidth / options.inSampleSize > 4096 || options.outHeight / options.inSampleSize > 4096) options.inSampleSize *= 2
    options.inJustDecodeBounds = false
    return checkNotNull(BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)).asImageBitmap()
}


internal fun resolveReaderLink(file: DesktopFile, link: String): String? {
    if (file.remote) return null
    val path = Uri.decode(link.substringBefore('#')).replace('\\', '/')
    if (path.isBlank() || path.any(Char::isISOControl)) return null
    val drive = Regex("^[A-Za-z]:/")
    if (':' in path && !drive.containsMatchIn(path)) return null
    val base = file.path.replace('\\', '/')
    val absolute = if (path.startsWith('/') || drive.containsMatchIn(path)) path else "${base.substringBeforeLast('/')}/$path"
    val prefix = if (absolute.startsWith("//")) "//" else if (absolute.startsWith('/')) "/" else ""
    val segments = mutableListOf<String>()
    val minimum = if (prefix == "//") 2 else if (drive.containsMatchIn(absolute)) 1 else 0
    for (part in absolute.removePrefix(prefix).split('/')) when (part) {
        "", "." -> Unit
        ".." -> if (segments.size > minimum) segments.removeAt(segments.lastIndex)
        else -> segments += part
    }
    return prefix + segments.joinToString("/")
}
