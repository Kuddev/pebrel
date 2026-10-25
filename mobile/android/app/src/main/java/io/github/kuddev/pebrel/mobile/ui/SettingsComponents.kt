package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

/** A settings category owns its outer surface; rows own only their input state. */
@Composable
internal fun SettingsGroup(title: String, content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(14.dp)
    Column(Modifier.fillMaxWidth().padding(top = 20.dp, bottom = 10.dp)) {
        Text(title, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 4.dp, bottom = 10.dp))
        Column(Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surface)
            .border(.5.dp, MaterialTheme.colorScheme.outlineVariant, shape), content = content)
    }
}

@Composable
internal fun SettingDivider() {
    HorizontalDivider(Modifier.padding(start = 48.dp), thickness = .5.dp,
        color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = .65f))
}

@Composable
internal fun SettingsRow(@DrawableRes icon: Int, title: String, detail: String = "", onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 60.dp)
        .padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Glyph(icon)
        Text(title, fontSize = 14.sp, modifier = Modifier.weight(1f))
        if (detail.isNotEmpty()) Text(detail, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 130.dp))
        Glyph(R.drawable.ic_chevron, Modifier.size(14.dp))
    }
}

@Composable
internal fun SettingsChoice(title: String, selected: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().selectable(selected, role = Role.RadioButton, onClick = onClick)
        .heightIn(min = 58.dp).padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, Modifier.weight(1f), fontSize = 14.sp)
        Box(Modifier.size(20.dp)) { if (selected) Glyph(R.drawable.ic_check, color = MaterialTheme.colorScheme.primary) }
    }
}

@Composable
internal fun TogglePreference(title: String, checked: Boolean, enabled: Boolean = true, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().toggleable(checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)
        .heightIn(min = 60.dp).padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, Modifier.weight(1f).padding(end = 12.dp), fontSize = 14.sp)
        Switch(checked, onCheckedChange = null, enabled = enabled)
    }
}
