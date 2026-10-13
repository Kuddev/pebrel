package io.github.kuddev.pebrel.mobile.ui

import android.net.Uri
import android.text.format.Formatter
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextOverflow
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.SftpTab
import io.github.kuddev.pebrel.mobile.connection.SftpEntry
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlin.math.abs

internal fun sftpSelectionRange(visiblePaths: List<String>, anchor: String?, target: String): Set<String> {
    val end = visiblePaths.indexOf(target)
    if (end < 0) return emptySet()
    val start = visiblePaths.indexOf(anchor).takeIf { it >= 0 } ?: end
    // 以当前可见排序计算范围，隐藏文件和父目录占位行不应被意外选中。
    return visiblePaths.subList(minOf(start, end), maxOf(start, end) + 1).toSet()
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SftpSwipeRow(selected: Boolean?, enabled: Boolean, onOpen: () -> Unit,
                         onLongClick: (() -> Unit)?, onSwipe: () -> Unit,
                         content: @Composable RowScope.() -> Unit) {
    val motion = rememberPebrelMotion()
    val haptic = LocalHapticFeedback.current
    val swipe = rememberUpdatedState {
        onSwipe()
        // 确认一次有效滑动只反馈一次，短滑、取消和回弹不重复震动；遵循系统触感设置。
        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
    }
    var dragging by remember { mutableStateOf(false) }
    var offset by remember { mutableFloatStateOf(0f) }
    val settledOffset by animateFloatAsState(offset, if (dragging) snap() else motion.tweenOrSnap(160), label = "file-swipe")
    // 跟手阶段直接绘制当前位移，不等待动画采样；仅松手时做有界回弹。
    val translation = if (dragging) offset else settledOffset
    val colors = MaterialTheme.colorScheme
    // 选中态是连续色带；按压态仍按未选中条目的圆角裁剪，不把两种状态混为一谈。
    Box(Modifier.fillMaxWidth().clip(if (selected == true) RectangleShape else MaterialTheme.shapes.medium)
        .background(if (selected == true) colors.primary.copy(alpha = .10f) else Color.Transparent)
        .semantics { if (selected != null) this.selected = selected }
        .pointerInput(enabled) {
            var distance = 0f
            detectHorizontalDragGestures(
                onDragStart = { distance = 0f; dragging = enabled },
                onDragCancel = { dragging = false; offset = 0f },
                onDragEnd = {
                    if (enabled && abs(distance) >= 48.dp.toPx()) swipe.value()
                    dragging = false; offset = 0f
                },
            ) { change, amount ->
                if (enabled) {
                    distance += amount
                    offset = distance.coerceIn(-96.dp.toPx(), 96.dp.toPx())
                    change.consume()
                }
            }
        }
        .combinedClickable(enabled = enabled, onClick = onOpen, onLongClick = onLongClick)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).graphicsLayer { translationX = translation }
            .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically, content = content)
    }
}

@Composable
internal fun SftpParentRow(enabled: Boolean, onOpen: () -> Unit) {
    // 与文件共用跟手和触感，唯独提交动作是返回上级，永远没有多选语义。
    SftpSwipeRow(null, enabled, onOpen, null, onOpen) {
        FileSymbol("..", directory = true, modifier = Modifier.padding(end = 12.dp).size(22.dp))
        Text("..", fontSize = 14.sp)
    }
}

@Composable
internal fun SftpEntryRow(entry: SftpEntry, selected: Boolean, enabled: Boolean, menuOpen: Boolean,
                         onMenu: (Boolean) -> Unit, onOpen: () -> Unit, onSwipe: () -> Unit,
                         onRename: () -> Unit, onDelete: () -> Unit) {
    val context = LocalContext.current
    val colors = MaterialTheme.colorScheme
    SftpSwipeRow(selected, enabled, onOpen, { onMenu(true) }, onSwipe) {
        FileSymbol(entry.path, entry.directory, Modifier.padding(end = 12.dp).size(22.dp))
        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
            Text(entry.name, fontSize = 14.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
            val updated = remember(entry.modified) {
                entry.modified?.let { java.text.SimpleDateFormat("yyyy-MM-dd HH:mm", java.util.Locale.getDefault())
                    .format(java.util.Date(it * 1000)) } ?: "—"
            }
            Row(Modifier.fillMaxWidth().padding(end = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(updated, Modifier.weight(1f, fill = false), color = colors.onSurfaceVariant,
                    fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (!entry.directory && entry.size != null) Text(Formatter.formatShortFileSize(context, entry.size),
                    color = colors.onSurfaceVariant, fontSize = 12.sp, maxLines = 1)
            }
        }
        Box {
            GlyphButton(R.drawable.ic_more, stringResource(R.string.sftp_file_actions, entry.name), { onMenu(true) }, enabled)
            DropdownMenu(menuOpen, { onMenu(false) }, Modifier.widthIn(min = 180.dp, max = 280.dp)) {
                Text(entry.name, Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                    style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant,
                    maxLines = 2, overflow = TextOverflow.Ellipsis)
                HorizontalDivider()
                DropdownMenuItem({ Text(stringResource(R.string.sftp_open)) }, { onMenu(false); onOpen() }, enabled = enabled)
                DropdownMenuItem({ Text(stringResource(R.string.sftp_rename)) }, { onMenu(false); onRename() }, enabled = enabled)
                DropdownMenuItem({ Text(stringResource(R.string.sftp_delete), color = colors.error) }, { onMenu(false); onDelete() }, enabled = enabled)
            }
        }
    }
}

@Composable
internal fun SftpTabRow(tab: SftpTab, host: String, selected: Boolean, onOpen: () -> Unit, onClose: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
        .background(if (selected) MaterialTheme.colorScheme.surfaceVariant else MaterialTheme.colorScheme.surface), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f).heightIn(min = 56.dp).clickable(onClick = onOpen).padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            FileSymbol(tab.file.path, modifier = Modifier.size(20.dp))
            Column(Modifier.weight(1f).padding(horizontal = 12.dp, vertical = 8.dp)) {
                Text(tab.file.name, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text("SFTP · $host", color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        GlyphButton(R.drawable.ic_close, stringResource(R.string.tab_close_named, tab.file.name), onClose)
    }
}

@Composable
internal fun SftpTransferProgress(count: Long, total: Long?, cancelling: Boolean = false, onCancel: () -> Unit) {
    val context = LocalContext.current
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            val amount = Formatter.formatShortFileSize(context, count)
            HelperText(if (cancelling) stringResource(R.string.sftp_cancelling)
                else if (total != null) "$amount / ${Formatter.formatShortFileSize(context, total)}" else amount, Modifier.weight(1f))
            TextButton(onCancel, enabled = !cancelling) { Text(stringResource(R.string.cancel)) }
        }
        if (total != null) LinearProgressIndicator(progress = { if (total == 0L) 1f else (count.toFloat() / total).coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
        else LinearProgressIndicator(Modifier.fillMaxWidth())
    }
}

@Composable
internal fun SftpFailureRow(code: String, onRetry: (() -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(sftpFailureText(code), Modifier.weight(1f), color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
        if (onRetry != null) TextButton(onRetry) { Text(stringResource(R.string.sftp_refresh)) }
    }
}

internal fun sftpFailureCode(error: Exception): String = when (error) {
    is NativeSshException -> error.code
    is IllegalArgumentException -> "INVALID_INPUT"
    is java.io.IOException, is SecurityException -> "SFTP_LOCAL_FILE"
    else -> "SFTP_OPERATION"
}

@Composable
internal fun sftpFailureText(code: String): String = stringResource(when (code) {
    "SFTP_NOT_FOUND" -> R.string.sftp_not_found
    "SFTP_PERMISSION" -> R.string.sftp_permission
    "SFTP_EXISTS" -> R.string.sftp_exists
    "SFTP_UNSUPPORTED" -> R.string.sftp_unsupported
    "SFTP_STALE", "SFTP_CHANGED" -> R.string.sftp_changed
    "SFTP_CLOSED", "CLOSED", "NETWORK" -> R.string.sftp_disconnected
    "SFTP_TIMEOUT", "TIMEOUT" -> R.string.sftp_timeout
    "SFTP_LIMIT", "SFTP_DIRECTORY_LIMIT" -> R.string.sftp_limit
    "SFTP_TOO_LARGE" -> R.string.sftp_too_large
    "SFTP_FILE_TYPE", "SFTP_BINARY" -> R.string.sftp_binary
    "SFTP_LOCAL_FILE" -> R.string.sftp_local_file
    "INVALID_INPUT" -> R.string.sftp_invalid_path
    "image_decode_failed" -> R.string.reader_image_failed
    "clipboard_failed" -> R.string.reader_copy_failed
    "link_open_failed" -> R.string.reader_link_failed
    else -> R.string.sftp_operation_failed
})

internal fun isSftpImage(path: String): Boolean = path.substringAfterLast('.').lowercase() in setOf("png", "jpg", "jpeg", "webp", "gif", "bmp")

internal fun resolveSftpLink(base: String, link: String): String? {
    if (Uri.parse(link).scheme != null) return null
    val path = Uri.decode(link.substringBefore('#'))
    if (path.isBlank() || path.any(Char::isISOControl)) return null
    val absolute = if (path.startsWith('/')) path else "${base.substringBeforeLast('/')}/$path"
    val parts = mutableListOf<String>()
    for (part in absolute.split('/')) when (part) {
        "", "." -> Unit
        ".." -> if (parts.isNotEmpty()) parts.removeAt(parts.lastIndex)
        else -> parts += part
    }
    return "/" + parts.joinToString("/")
}
