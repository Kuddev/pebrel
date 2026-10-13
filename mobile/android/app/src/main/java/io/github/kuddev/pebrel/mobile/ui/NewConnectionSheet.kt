package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewConnectionSheet(
    onDismiss: () -> Unit,
    onComputer: () -> Unit,
    onSshHost: () -> Unit,
    onLocal: () -> Unit,
    onDeployRelay: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp)
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var choosing by remember { mutableStateOf(false) }
    fun choose(action: () -> Unit) {
        if (choosing) return
        choosing = true
        scope.launch {
            try {
                // 先收起面板再打开表单，避免两个模态窗口同时争抢焦点。
                sheet.hide()
                onDismiss()
                action()
            } finally {
                choosing = false
            }
        }
    }
    // 圆角交给组件内部 Surface；外层描边不会随面板位移，会残留在屏幕顶部。
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheet,
        shape = shape,
        containerColor = colors.surface,
        contentColor = colors.onSurface,
        tonalElevation = 0.dp,
        scrimColor = colors.scrim.copy(alpha = .62f),
        dragHandle = {
            Box(Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 8.dp), contentAlignment = Alignment.Center) {
                Box(Modifier.size(32.dp, 4.dp).background(colors.onSurfaceVariant.copy(alpha = .4f), RoundedCornerShape(2.dp)))
            }
        },
    ) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 16.dp)) {
            Text(stringResource(R.string.new_connection), fontSize = 13.sp, color = colors.onSurfaceVariant,
                modifier = Modifier.padding(start = 18.dp, top = 4.dp, bottom = 12.dp).semantics { heading() })
            NewConnectionRow(R.drawable.ic_monitor, stringResource(R.string.computer_connect),
                stringResource(R.string.new_computer_hint), !choosing) { choose(onComputer) }
            NewConnectionRow(R.drawable.ic_terminal, stringResource(R.string.add_ssh),
                stringResource(R.string.new_ssh_hint), !choosing) { choose(onSshHost) }
            NewConnectionRow(R.drawable.ic_phone, stringResource(R.string.local_terminal),
                enabled = !choosing) { choose(onLocal) }
            NewConnectionRow(R.drawable.ic_sliders, stringResource(R.string.new_deploy_relay),
                stringResource(R.string.new_deploy_relay_hint), !choosing) { choose(onDeployRelay) }
        }
    }
}

@Composable
private fun NewConnectionRow(
    @DrawableRes icon: Int,
    title: String,
    detail: String? = null,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Row(Modifier.fillMaxWidth().clickable(enabled = enabled, role = Role.Button, onClick = onClick)
        .heightIn(min = 60.dp).padding(horizontal = 28.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Glyph(icon, Modifier.size(20.dp))
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 15.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium)
            if (detail != null) Text(detail, fontSize = 12.sp, lineHeight = 18.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 2.dp))
        }
    }
}
