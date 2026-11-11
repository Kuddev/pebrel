package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R

/** One pane identity, two views. Material owns selection, focus and touch semantics. */
@Composable
internal fun SessionModeTabs(conversation: Boolean, onSwitch: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        listOf(R.string.chat_title, R.string.chat_terminal).forEachIndexed { index, label ->
            val selected = conversation == (index == 0)
            SegmentedButton(selected = selected, onClick = { if (!selected) onSwitch() },
                shape = SegmentedButtonDefaults.itemShape(index, 2, MaterialTheme.shapes.small),
                modifier = Modifier.heightIn(min = 48.dp), icon = {},
                colors = SegmentedButtonDefaults.colors(
                    activeContainerColor = colors.surfaceVariant, activeContentColor = colors.onSurface,
                    inactiveContainerColor = colors.background, inactiveContentColor = colors.onSurfaceVariant,
                    activeBorderColor = colors.outlineVariant, inactiveBorderColor = colors.outlineVariant,
                )) { Text(stringResource(label), style = MaterialTheme.typography.labelLarge) }
        }
    }
}
