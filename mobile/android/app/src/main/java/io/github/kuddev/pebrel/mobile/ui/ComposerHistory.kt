package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import io.github.kuddev.pebrel.mobile.R

private class AboveAnchorPositionProvider(private val gap: Int) : PopupPositionProvider {
    override fun calculatePosition(
        anchorBounds: IntRect,
        windowSize: IntSize,
        layoutDirection: LayoutDirection,
        popupContentSize: IntSize,
    ): IntOffset {
        val centered = anchorBounds.left + (anchorBounds.width - popupContentSize.width) / 2
        val x = centered.coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0))
        val above = anchorBounds.top - popupContentSize.height - gap
        val below = anchorBounds.bottom + gap
        val y = if (above >= 0) above else below.coerceAtMost((windowSize.height - popupContentSize.height).coerceAtLeast(0))
        return IntOffset(x, y)
    }
}

@Composable
internal fun ComposerHistory(
    commands: List<String>,
    selected: String?,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
    maxHeight: Dp = 208.dp,
    onDismiss: () -> Unit = {},
) {
    if (commands.isEmpty()) return
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(14.dp)
    val historyLabel = stringResource(R.string.composer_history)
    val density = androidx.compose.ui.platform.LocalDensity.current
    Popup(
        popupPositionProvider = AboveAnchorPositionProvider(with(density) { 8.dp.roundToPx() }),
        properties = PopupProperties(focusable = false, dismissOnClickOutside = true, dismissOnBackPress = false),
        onDismissRequest = onDismiss,
    ) {
        Column(
            modifier.fillMaxWidth().heightIn(max = maxHeight)
                .clip(shape)
                .background(colors.surfaceVariant.copy(alpha = .50f))
                .border(.5.dp, colors.outlineVariant.copy(alpha = .75f), shape)
                .semantics { contentDescription = historyLabel },
        ) {
            LazyColumn(
                modifier = Modifier.fillMaxWidth().heightIn(max = maxHeight),
                verticalArrangement = Arrangement.spacedBy(1.dp),
            ) {
                items(commands, key = { it }) { command ->
                    val interactions = remember(command) { MutableInteractionSource() }
                    val pressed by interactions.collectIsPressedAsState()
                    val chosen = selected == command
                    val fill = when {
                        chosen -> colors.primary.copy(alpha = .14f)
                        pressed -> colors.primary.copy(alpha = .08f)
                        else -> Color.Transparent
                    }
                    Row(
                        Modifier.fillMaxWidth()
                            .heightIn(min = 52.dp)
                            .clip(RoundedCornerShape(11.dp))
                            .background(fill)
                            .clickable(
                                interactionSource = interactions,
                                indication = null,
                                role = Role.Button,
                                onClick = { onSelect(command) },
                            )
                            .padding(horizontal = 13.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        androidx.compose.material3.Icon(
                            painterResource(R.drawable.ic_history),
                            contentDescription = null,
                            modifier = Modifier.size(17.dp),
                            tint = colors.onSurfaceVariant,
                        )
                        Spacer(Modifier.width(10.dp))
                        Text(
                            command,
                            modifier = Modifier.weight(1f),
                            fontFamily = LocalTerminalFont.current,
                            fontSize = 12.sp,
                            lineHeight = 17.sp,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            color = colors.onSurface,
                        )
                        if (chosen) {
                            Spacer(Modifier.width(8.dp))
                            androidx.compose.material3.Icon(
                                painterResource(R.drawable.ic_check),
                                contentDescription = stringResource(R.string.composer_history_filled),
                                modifier = Modifier.size(16.dp),
                                tint = colors.primary,
                            )
                        }
                    }
                }
            }
        }
    }
}
