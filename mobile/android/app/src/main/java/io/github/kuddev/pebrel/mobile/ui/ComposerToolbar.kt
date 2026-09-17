package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

@Composable
internal fun ComposerIconButton(
    @DrawableRes icon: Int,
    label: String,
    enabled: Boolean = true,
    selected: Boolean = false,
    selectedContainer: Color? = null,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    modifier: Modifier = Modifier,
    busy: Boolean = false,
    onClick: () -> Unit,
) {
    val interactions = remember { MutableInteractionSource() }
    val pressed by interactions.collectIsPressedAsState()
    val focused by interactions.collectIsFocusedAsState()
    val hovered by interactions.collectIsHoveredAsState()
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(12.dp)
    val fill = when {
        !enabled -> Color.Transparent
        selected -> selectedContainer ?: colors.primary.copy(alpha = .14f)
        pressed -> colors.primary.copy(alpha = .10f)
        hovered -> colors.primary.copy(alpha = .06f)
        else -> Color.Transparent
    }
    val animatedFill by animateColorAsState(fill, rememberPebrelMotion().tweenOrSnap(120), label = "composer-action")
    val iconTint = if (enabled) tint else colors.onSurface.copy(alpha = .38f)
    Box(
        modifier
            .size(48.dp)
            .clip(shape)
            .background(animatedFill)
            .then(if (focused && enabled) Modifier.border(1.dp, colors.primary.copy(alpha = .48f), shape) else Modifier)
            .clickable(
                interactionSource = interactions,
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            )
            .semantics {
                contentDescription = label
                if (selected) this.selected = true
            },
        contentAlignment = Alignment.Center,
    ) {
        if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 1.5.dp, color = iconTint)
        else Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(19.dp), tint = iconTint)
    }
}

@Composable
internal fun ComposerToolbar(
    onEdit: (() -> Unit)?,
    keyboardVisible: Boolean,
    keyboardEnabled: Boolean,
    onImeToggle: () -> Unit,
    enabled: Boolean,
    shortcuts: List<String> = emptyList(),
    onKey: ((String) -> Unit)? = null,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().testTag("composer-direct")
            .heightIn(min = 52.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(colors.surfaceVariant.copy(alpha = .30f))
            .padding(horizontal = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onKey != null && shortcuts.isNotEmpty()) {
            ComposerShortcutRow(
                shortcuts = shortcuts,
                enabled = enabled,
                onKey = onKey,
                modifier = Modifier.weight(1f),
                surface = false,
            )
        } else {
            Spacer(Modifier.weight(1f))
        }
        Box(Modifier.padding(horizontal = 4.dp).width(1.dp).height(24.dp)
            .background(colors.outlineVariant.copy(alpha = .45f)))
        onEdit?.let { edit ->
            ComposerIconButton(
                icon = R.drawable.ic_compose_bubble,
                label = stringResourceCompat(R.string.composer_mode_edit),
                tint = colors.onSurfaceVariant,
                onClick = edit,
            )
        }
        ComposerIconButton(
            icon = R.drawable.ic_keyboard,
            label = stringResourceCompat(
                if (keyboardVisible) R.string.composer_hide_keyboard else R.string.composer_show_keyboard,
            ),
            enabled = enabled && keyboardEnabled,
            selected = keyboardVisible,
            onClick = onImeToggle,
        )
    }
}

@Composable
private fun stringResourceCompat(id: Int): String = androidx.compose.ui.res.stringResource(id)

@Composable
internal fun ComposerShortcutRow(
    shortcuts: List<String>,
    enabled: Boolean,
    onKey: (String) -> Unit,
    modifier: Modifier = Modifier,
    surface: Boolean = true,
) {
    if (shortcuts.isEmpty()) return
    val scroll = rememberScrollState()
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(14.dp)
    Box(
        modifier.fillMaxWidth()
            .heightIn(min = if (surface) 52.dp else 48.dp)
            .clipToBounds()
            .then(if (surface) Modifier.clip(shape).background(colors.surfaceVariant.copy(alpha = .22f)) else Modifier)
            .drawWithContent {
                drawContent()
                val fadeColor = colors.surfaceVariant.copy(alpha = if (surface) .22f else .30f)
                    .compositeOver(colors.background)
                val edge = 24.dp.toPx().coerceAtMost(size.width / 3f)
                if (scroll.canScrollBackward && edge > 0f) {
                    drawRect(
                        Brush.horizontalGradient(
                            colors = listOf(fadeColor, Color.Transparent),
                            startX = 0f,
                            endX = edge,
                        ),
                        size = androidx.compose.ui.geometry.Size(edge, size.height),
                    )
                }
                if (scroll.canScrollForward && edge > 0f) {
                    drawRect(
                        Brush.horizontalGradient(
                            colors = listOf(Color.Transparent, fadeColor),
                            startX = size.width - edge,
                            endX = size.width,
                        ),
                        topLeft = androidx.compose.ui.geometry.Offset(size.width - edge, 0f),
                        size = androidx.compose.ui.geometry.Size(edge, size.height),
                    )
                }
            },
    ) {
        Row(
            Modifier.horizontalScroll(scroll).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            shortcuts.forEach { shortcut ->
                val interactions = remember(shortcut) { MutableInteractionSource() }
                val pressed by interactions.collectIsPressedAsState()
                val focused by interactions.collectIsFocusedAsState()
                val hovered by interactions.collectIsHoveredAsState()
                val fill = when {
                    pressed -> colors.primary.copy(alpha = .10f)
                    hovered -> colors.primary.copy(alpha = .06f)
                    else -> Color.Transparent
                }
                Box(
                    Modifier.heightIn(min = 48.dp).widthIn(min = 48.dp)
                        .clip(RoundedCornerShape(11.dp))
                        .background(fill)
                        .then(if (focused && enabled) Modifier.border(1.dp, colors.primary.copy(alpha = .48f), RoundedCornerShape(11.dp)) else Modifier)
                        .clickable(
                            interactionSource = interactions,
                            indication = null,
                            enabled = enabled,
                            role = Role.Button,
                            onClick = { onKey(shortcut) },
                        )
                        .semantics { contentDescription = shortcut }
                        .padding(horizontal = 12.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    androidx.compose.material3.Text(
                        shortcut,
                        fontFamily = LocalTerminalFont.current,
                        fontSize = 12.sp,
                        color = colors.onSurface.copy(alpha = if (enabled) 1f else .45f),
                    )
                }
            }
        }
    }
}
