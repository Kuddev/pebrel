package io.github.kuddev.pebrel.mobile.ui

import android.net.Uri
import android.text.format.Formatter
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextOverflow
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.SftpTab
import io.github.kuddev.pebrel.ssh.NativeSshException

@Composable
internal fun SftpTabRow(tab: SftpTab, host: String, selected: Boolean, onOpen: () -> Unit, onClose: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
        .background(if (selected) MaterialTheme.colorScheme.surfaceVariant else MaterialTheme.colorScheme.surface), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f).heightIn(min = 56.dp).clickable(onClick = onOpen).padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Glyph(if (isSftpImage(tab.file.path)) R.drawable.ic_image else R.drawable.ic_git_file, Modifier.size(18.dp))
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
