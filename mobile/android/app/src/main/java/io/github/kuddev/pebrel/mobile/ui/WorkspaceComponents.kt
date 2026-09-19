package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.animation.animateContentSize
import androidx.annotation.DrawableRes
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

/** One outlined surface treatment for hosts, devices and terminal workspaces. */
@Composable
fun Modifier.workspaceFrame(): Modifier {
    val shape = RoundedCornerShape(14.dp)
    val colors = MaterialTheme.colorScheme
    return clip(shape).background(colors.surface).border(1.dp, colors.outlineVariant, shape)
}

@Composable
fun WorkspaceSymbol(content: @Composable () -> Unit) {
    Box(Modifier.size(44.dp).clip(RoundedCornerShape(11.dp))
        .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .45f)),
        contentAlignment = Alignment.Center) { content() }
}

/** Geometry shared by the native pages, matching the approved mobile prototype. */
@Composable
fun Glyph(@DrawableRes icon: Int, modifier: Modifier = Modifier, color: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Icon(painterResource(icon), contentDescription = null, modifier = modifier.size(20.dp), tint = color)
}

@Composable
fun GlyphButton(@DrawableRes icon: Int, label: String, onClick: () -> Unit, enabled: Boolean = true) {
    IconButton(onClick, enabled = enabled, modifier = Modifier.size(48.dp)) {
        Icon(painterResource(icon), label, modifier = Modifier.size(20.dp))
    }
}

@Composable
fun PageHeader(title: String, onBack: () -> Unit, actions: @Composable RowScope.() -> Unit = {}) {
    Row(Modifier.fillMaxWidth().height(67.dp).background(MaterialTheme.colorScheme.surface).padding(horizontal = 9.dp),
        verticalAlignment = Alignment.CenterVertically) {
        GlyphButton(R.drawable.ic_back, stringResource(R.string.back), onBack)
        Text(title, modifier = Modifier.weight(1f).padding(start = 4.dp), fontSize = 20.sp,
            fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        actions()
    }
}

@Composable
fun GroupHeading(title: String, count: Int? = null, action: String? = null, onAction: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, fontSize = 13.sp, fontWeight = FontWeight.Medium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (count != null) Text(" $count", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.weight(1f))
        if (action != null) TextButton(onAction, contentPadding = PaddingValues(start = 8.dp)) {
            Text(action, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Glyph(R.drawable.ic_chevron, Modifier.padding(start = 5.dp).size(14.dp))
        }
    }
}

@Composable
fun statusLabel(status: String): String = stringResource(when (status) {
    "ready" -> R.string.state_ready
    "connecting" -> R.string.state_connecting
    "ended" -> R.string.state_ended
    "failed" -> R.string.state_failed
    "disconnected" -> R.string.state_disconnected
    "finished" -> R.string.state_finished
    "waiting_input", "attention" -> R.string.state_attention
    "running" -> R.string.state_running
    "idle" -> R.string.state_idle
    else -> R.string.state_unknown
})

@Composable
fun StatusCaption(status: String, prefix: String = "") {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        val color = when (status) {
            "ready", "finished" -> MaterialTheme.colorScheme.primary
            "failed" -> MaterialTheme.colorScheme.error
            else -> MaterialTheme.colorScheme.onSurfaceVariant
        }
        if (status == "connecting") CircularProgressIndicator(Modifier.size(10.dp), strokeWidth = 1.5.dp)
        else Box(Modifier.size(6.dp).background(color, CircleShape))
        Text(prefix + statusLabel(status), color = MaterialTheme.colorScheme.onSurfaceVariant,
            fontFamily = LocalTerminalFont.current, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
fun NavigationRow(@DrawableRes icon: Int, title: String, detail: String = "", onClick: () -> Unit) {
    val motion = rememberPebrelMotion()
    Row(Modifier.fillMaxWidth().animateContentSize(motion.contentSizeSpec()).padding(vertical = 4.dp).workspaceFrame().clickable(onClick = onClick)
        .heightIn(min = 64.dp).padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Glyph(icon)
        Text(title, fontSize = 14.sp, modifier = Modifier.weight(1f))
        if (detail.isNotEmpty()) Text(detail, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 125.dp))
        Glyph(R.drawable.ic_chevron, Modifier.size(14.dp))
    }
}

@Composable
fun HelperText(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier, fontSize = 12.sp, lineHeight = 20.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
