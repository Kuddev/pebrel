package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.*
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.kuddev.pebrel.mobile.R

/** Full-page sheet geometry from the approved HTML, with Android touch targets. */
@Composable
fun ConnectionForm(title: String, onClose: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    Dialog(onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().systemBarsPadding().imePadding().verticalScroll(rememberScrollState())
                .padding(start = 20.dp, end = 20.dp, top = 10.dp, bottom = 24.dp)) {
                ConnectionHeading(title, onClose)
                Spacer(Modifier.height(20.dp))
                content()
            }
        }
    }
}

@Composable
fun ConnectionHeading(title: String, onClose: () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 62.dp).padding(bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, fontSize = 19.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
        GlyphButton(R.drawable.ic_close, stringResource(R.string.close), onClose)
    }
    HorizontalDivider(thickness = .5.dp)
}

@Composable
fun ConnectionField(value: String, onChange: (String) -> Unit, label: Int, modifier: Modifier = Modifier,
                    keyboard: KeyboardType = KeyboardType.Text, placeholder: String = "", limit: Int = 253,
                    transformation: VisualTransformation = VisualTransformation.None, showLabel: Boolean = true) {
    val interactions = remember { MutableInteractionSource() }
    val focused by interactions.collectIsFocusedAsState()
    val colors = MaterialTheme.colorScheme
    val motion = rememberPebrelMotion()
    val shape = RoundedCornerShape(14.dp)
    val accessibleLabel = stringResource(label)
    val borderColor by animateColorAsState(
        targetValue = if (focused) colors.primary else colors.outlineVariant,
        animationSpec = motion.tweenOrSnap(140),
        label = "connection-field-border-color",
    )
    val borderWidth by animateDpAsState(
        targetValue = if (focused) 1.dp else .5.dp,
        animationSpec = motion.tweenOrSnap(140),
        label = "connection-field-border-width",
    )
    Column(modifier) {
        if (showLabel) {
            Text(accessibleLabel, fontSize = 11.sp, lineHeight = 16.sp, color = colors.onSurfaceVariant)
            Spacer(Modifier.height(7.dp))
        }
        BasicTextField(value, { if (it.length <= limit) onChange(it) },
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)
                .clip(shape).background(colors.surface).border(borderWidth, borderColor, shape)
                .semantics { contentDescription = accessibleLabel }
                .padding(horizontal = 11.dp, vertical = 10.dp),
            singleLine = true, interactionSource = interactions,
            textStyle = TextStyle(color = colors.onSurface, fontSize = 13.sp),
            cursorBrush = SolidColor(colors.primary), visualTransformation = transformation,
            keyboardOptions = KeyboardOptions(keyboardType = keyboard, autoCorrectEnabled = false),
            decorationBox = { field -> Box(contentAlignment = Alignment.CenterStart) {
                if (value.isEmpty()) Text(placeholder, fontSize = 13.sp, color = colors.onSurfaceVariant.copy(alpha = .6f))
                field()
            } })
    }
}

@Composable
fun ConnectionSearchField(value: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    val interactions = remember { MutableInteractionSource() }
    val focused by interactions.collectIsFocusedAsState()
    val colors = MaterialTheme.colorScheme
    val motion = rememberPebrelMotion()
    val shape = RoundedCornerShape(14.dp)
    val hint = stringResource(R.string.search_hosts)
    val borderColor by animateColorAsState(
        targetValue = if (focused) colors.primary else colors.outlineVariant,
        animationSpec = motion.tweenOrSnap(140),
        label = "connection-search-border-color",
    )
    val borderWidth by animateDpAsState(
        targetValue = if (focused) 1.dp else .5.dp,
        animationSpec = motion.tweenOrSnap(140),
        label = "connection-search-border-width",
    )
    BasicTextField(value, onChange,
        modifier = modifier.fillMaxWidth().heightIn(min = 48.dp)
            .clip(shape).background(colors.surface).border(borderWidth, borderColor, shape)
            .semantics { contentDescription = hint }
            .padding(horizontal = 12.dp, vertical = 10.dp),
        singleLine = true, interactionSource = interactions,
        textStyle = TextStyle(color = colors.onSurface, fontSize = 13.sp),
        cursorBrush = SolidColor(colors.primary),
        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
        decorationBox = { field ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Glyph(R.drawable.ic_search, Modifier.size(18.dp), colors.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
                Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                    if (value.isEmpty()) {
                        Text(hint, fontSize = 13.sp, color = colors.onSurfaceVariant.copy(alpha = .6f),
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    field()
                }
            }
        })
}

@Composable
fun ConnectionButton(label: String, enabled: Boolean = true, primary: Boolean = true, onClick: () -> Unit) {
    val modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)
    val shape = RoundedCornerShape(14.dp)
    if (primary) Button(onClick, modifier, enabled = enabled, shape = shape) { Text(label, fontSize = 13.sp) }
    else OutlinedButton(onClick, modifier, enabled = enabled, shape = shape) { Text(label, fontSize = 13.sp) }
}

@Composable
fun ConnectionSegments(options: List<Pair<String, String>>, value: String, onSelect: (String) -> Unit,
                       modifier: Modifier = Modifier, disabled: Set<String> = emptySet()) {
    if (options.isEmpty()) return
    val colors = MaterialTheme.colorScheme
    val motion = rememberPebrelMotion()
    val pill = RoundedCornerShape(50)
    // Keep the track quiet and let one shared indicator carry the selected state.
    // Each child keeps a 48 dp hit target while the visual track stays compact.
    BoxWithConstraints(
        modifier.selectableGroup().clip(pill)
            .drawBehind {
                val inset = 6.dp.toPx()
                val height = (size.height - inset * 2).coerceAtLeast(0f)
                drawRoundRect(colors.surfaceVariant.copy(alpha = .6f), Offset(0f, inset),
                    Size(size.width, height), CornerRadius(height / 2))
            }
            .padding(horizontal = 3.dp),
    ) {
        val gap = 2.dp
        val slotWidth = ((maxWidth - gap * (options.size - 1)).coerceAtLeast(0.dp)) / options.size
        val selectedIndex = options.indexOfFirst { it.first == value }.coerceAtLeast(0)
        val targetOffset = (slotWidth + gap) * selectedIndex
        val indicatorOffset by animateDpAsState(
            targetValue = targetOffset,
            animationSpec = motion.tweenOrSnap(180),
            label = "connection-segment-indicator-offset",
        )

        // The indicator is measured against the actual row height. Its 8 dp
        // vertical inset is the 6 dp track inset plus 2 dp of inner breathing room.
        Box(Modifier.fillMaxWidth()) {
            Box(Modifier.matchParentSize().padding(vertical = 8.dp)) {
                Box(Modifier.align(Alignment.CenterStart).offset(x = indicatorOffset)
                    .width(slotWidth).fillMaxHeight().clip(pill).background(colors.background))
            }
            Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), horizontalArrangement = Arrangement.spacedBy(gap)) {
                options.forEach { (id, label) ->
                    val selected = id == value
                    val enabled = id !in disabled
                    val interactions = remember(id) { MutableInteractionSource() }
                    val pressed by interactions.collectIsPressedAsState()
                    val focused by interactions.collectIsFocusedAsState()
                    val hovered by interactions.collectIsHoveredAsState()
                    Box(Modifier.weight(1f).heightIn(min = 48.dp).clip(pill)
                        .clickable(
                            interactionSource = interactions,
                            indication = null,
                            enabled = enabled,
                            role = Role.RadioButton,
                        ) { onSelect(id) }
                        .drawBehind {
                            if (pressed && enabled) {
                                val inset = 6.dp.toPx()
                                val height = (size.height - inset * 2).coerceAtLeast(0f)
                                drawRoundRect(colors.primary.copy(alpha = .10f), Offset(0f, inset),
                                    Size(size.width, height), CornerRadius(height / 2))
                            } else if (hovered && enabled) {
                                val inset = 6.dp.toPx()
                                val height = (size.height - inset * 2).coerceAtLeast(0f)
                                drawRoundRect(colors.primary.copy(alpha = .05f), Offset(0f, inset),
                                    Size(size.width, height), CornerRadius(height / 2))
                            }
                            if (focused && enabled) {
                                val stroke = 1.dp.toPx()
                                val inset = stroke / 2
                                drawRoundRect(colors.primary.copy(alpha = .48f), Offset(inset, inset),
                                    Size(size.width - stroke, size.height - stroke), CornerRadius((size.height - stroke) / 2),
                                    style = androidx.compose.ui.graphics.drawscope.Stroke(stroke))
                            }
                        }
                        .semantics { this.selected = selected }
                        .padding(horizontal = 9.dp, vertical = 12.dp),
                        contentAlignment = Alignment.Center) {
                        Text(label, fontSize = 12.sp, lineHeight = 18.sp,
                            fontWeight = if (selected) FontWeight.Medium else FontWeight.Normal,
                            color = (if (selected) colors.onSurface else colors.onSurfaceVariant)
                                .copy(alpha = if (enabled) 1f else .45f))
                    }
                }
            }
        }
    }
}
