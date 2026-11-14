package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.TextToolbar
import androidx.compose.ui.platform.TextToolbarStatus
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.ConversationPrompt
import kotlinx.coroutines.delay

/** A new question ignores taps this long, so a touch meant for the previous one cannot answer it. */
internal const val PROMPT_GUARD_MS = 350L

/**
 * Snapshots announce state changes and wake the reader at once. Output appended while the Agent
 * works does not change the snapshot, so it is still polled: sooner while pages keep changing.
 */
internal fun conversationPollDelay(state: String, changed: Boolean): Long = when {
    state != "running" -> 3000L
    changed -> 800L
    else -> 1500L
}

internal fun elapsedLabelSeconds(startedAt: Long?, now: Long): Long? =
    startedAt?.let { ((now - it) / 1000).coerceAtLeast(0) }

/** Records whether a selection toolbar is showing; the list stops following new output meanwhile. */
internal class ObservingTextToolbar(private val platform: TextToolbar) : TextToolbar {
    var selecting by mutableStateOf(false)
        private set
    override val status: TextToolbarStatus get() = platform.status

    override fun hide() {
        platform.hide()
        selecting = false
    }

    override fun showMenu(rect: Rect, onCopyRequested: (() -> Unit)?, onPasteRequested: (() -> Unit)?,
                          onCutRequested: (() -> Unit)?, onSelectAllRequested: (() -> Unit)?) {
        platform.showMenu(rect, onCopyRequested, onPasteRequested, onCutRequested, onSelectAllRequested)
        selecting = true
    }

    override fun showMenu(rect: Rect, onCopyRequested: (() -> Unit)?, onPasteRequested: (() -> Unit)?,
                          onCutRequested: (() -> Unit)?, onSelectAllRequested: (() -> Unit)?,
                          onAutofillRequested: (() -> Unit)?) {
        platform.showMenu(rect, onCopyRequested, onPasteRequested, onCutRequested, onSelectAllRequested, onAutofillRequested)
        selecting = true
    }
}

/** Scrolls to the true end, including the bottom of a final message taller than the viewport. */
internal suspend fun LazyListState.scrollToEnd() {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    scrollToItem(last)
    while (canScrollForward) {
        if (scrollBy(10_000f) == 0f) break
    }
}

/** The desktop's projected question: options are bound to its prompt id, and stale ids are refused. */
@Composable
internal fun PromptCard(prompt: ConversationPrompt, enabled: Boolean, onChoose: (Int) -> Unit) {
    val colors = MaterialTheme.colorScheme
    var armed by remember(prompt.id) { mutableStateOf(false) }
    LaunchedEffect(prompt.id) {
        delay(PROMPT_GUARD_MS)
        armed = true
    }
    val labels = if (prompt.binary) listOf(stringResource(R.string.chat_allow), stringResource(R.string.chat_deny)) else prompt.options
    Column(Modifier.fillMaxWidth().glass(RoundedCornerShape(20.dp), GlassTier.Heavy, GlassLift.Floating)
        .heightIn(max = 340.dp).verticalScroll(rememberScrollState()).padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(stringResource(R.string.chat_question), style = MaterialTheme.typography.labelMedium, color = colors.primary)
        Text(prompt.text, style = MaterialTheme.typography.titleSmall, color = colors.onSurface)
        labels.forEachIndexed { index, label ->
            val selected = !prompt.binary && index == prompt.selected
            val shape = RoundedCornerShape(12.dp)
            Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(shape)
                .background(if (selected) colors.primary.copy(alpha = .1f) else colors.onSurface.copy(alpha = .04f))
                .pressClickable(enabled = enabled && armed, role = Role.Button) { onChoose(index) }
                .semantics { if (selected) this.selected = true }
                .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OptionNumber(index + 1, selected)
                Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium,
                    color = colors.onSurface.copy(alpha = if (enabled) 1f else .5f))
            }
        }
    }
}

@Composable
internal fun ConversationStatusLine(state: String, label: String, elapsedSeconds: Long?, canInterrupt: Boolean,
                                    onInterrupt: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val dot = when (state) {
        "running" -> colors.primary
        "failed" -> colors.error
        else -> colors.onSurfaceVariant
    }
    Row(modifier.glass(RoundedCornerShape(50), GlassTier.Light, GlassLift.Raised).heightIn(min = 40.dp)
        .padding(start = 14.dp, end = if (canInterrupt) 4.dp else 14.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(7.dp).background(dot, CircleShape))
        val elapsed = elapsedSeconds?.let { seconds ->
            if (seconds < 60) stringResource(R.string.chat_elapsed_seconds, seconds)
            else stringResource(R.string.chat_elapsed_minutes, seconds / 60, seconds % 60)
        }
        Text(listOfNotNull(label, elapsed).joinToString(" · "), style = MaterialTheme.typography.labelLarge,
            color = colors.onSurface, maxLines = 1)
        if (canInterrupt) TextButton(onInterrupt) { Text(stringResource(R.string.chat_interrupt)) }
    }
}

/** A compact notice on glass, so it stays legible over the scrolling transcript. */
@Composable
internal fun ConversationNotice(text: String, error: Boolean = false, action: String? = null, onAction: () -> Unit = {}) {
    val colors = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().glass(RoundedCornerShape(14.dp), GlassTier.Heavy, GlassLift.Raised)
        .heightIn(min = 44.dp).padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Text(text, Modifier.weight(1f).padding(vertical = 8.dp), style = MaterialTheme.typography.bodySmall,
            color = if (error) colors.error else colors.onSurfaceVariant)
        if (action != null) TextButton(onAction) { Text(action) }
    }
}

@Composable
internal fun BackToLatest(unseen: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val label = stringResource(R.string.chat_latest)
    Row(modifier.glass(RoundedCornerShape(50), GlassTier.Heavy, GlassLift.Floating)
        .pressClickable(role = Role.Button, onClickLabel = label, onClick = onClick)
        .heightIn(min = 40.dp).padding(horizontal = 14.dp)
        .semantics { contentDescription = label },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Icon(painterResource(R.drawable.ic_down), null, Modifier.size(14.dp), tint = colors.onSurface)
        if (unseen) Text(stringResource(R.string.chat_new_messages), style = MaterialTheme.typography.labelLarge,
            color = colors.primary)
    }
}
