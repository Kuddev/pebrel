package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.VisualTransformation
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
                    transformation: VisualTransformation = VisualTransformation.None) {
    val interactions = remember { MutableInteractionSource() }
    val focused by interactions.collectIsFocusedAsState()
    val colors = MaterialTheme.colorScheme
    val accessibleLabel = stringResource(label)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Text(stringResource(label), fontSize = 11.sp, lineHeight = 16.sp, color = colors.onSurfaceVariant)
        BasicTextField(value, { if (it.length <= limit) onChange(it) },
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).semantics { contentDescription = accessibleLabel }
                .background(colors.surface, MaterialTheme.shapes.small)
                .border(if (focused) 1.dp else .5.dp, if (focused) colors.primary else colors.outlineVariant, MaterialTheme.shapes.small)
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
fun ConnectionButton(label: String, enabled: Boolean = true, primary: Boolean = true, onClick: () -> Unit) {
    val modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)
    if (primary) Button(onClick, modifier, enabled = enabled, shape = MaterialTheme.shapes.medium) { Text(label, fontSize = 13.sp) }
    else OutlinedButton(onClick, modifier, enabled = enabled, shape = MaterialTheme.shapes.medium) { Text(label, fontSize = 13.sp) }
}

@Composable
fun ConnectionSegments(options: List<Pair<String, String>>, value: String, onSelect: (String) -> Unit,
                       modifier: Modifier = Modifier, disabled: Set<String> = emptySet()) {
    val colors = MaterialTheme.colorScheme
    val pill = RoundedCornerShape(50)
    // The visual track is 36 dp tall at normal text scale. The full 48 dp row
    // remains tappable, and larger accessibility text can increase its height.
    Row(modifier.selectableGroup().drawBehind {
        val inset = 6.dp.toPx()
        val height = size.height - inset * 2
        drawRoundRect(colors.surfaceVariant.copy(alpha = .6f), Offset(0f, inset),
            Size(size.width, height), CornerRadius(height / 2))
    }.padding(horizontal = 3.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        options.forEach { (id, label) ->
            val selected = id == value
            val enabled = id !in disabled
            Box(Modifier.weight(1f).heightIn(min = 48.dp).clip(pill)
                .selectable(selected, enabled = enabled, role = Role.RadioButton) { onSelect(id) }
                .drawBehind {
                    if (selected) {
                        val inset = 8.dp.toPx()
                        val height = size.height - inset * 2
                        drawRoundRect(colors.background, Offset(0f, inset), Size(size.width, height), CornerRadius(height / 2))
                    }
                }.padding(horizontal = 9.dp, vertical = 12.dp),
                contentAlignment = Alignment.Center) {
                Text(label, fontSize = 12.sp, lineHeight = 18.sp,
                    fontWeight = if (selected) FontWeight.Medium else FontWeight.Normal,
                    color = (if (selected) colors.onSurface else colors.onSurfaceVariant).copy(alpha = if (enabled) 1f else .45f))
            }
        }
    }
}
