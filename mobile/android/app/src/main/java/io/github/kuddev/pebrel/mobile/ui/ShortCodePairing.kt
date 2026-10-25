package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.border
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.PairingCodeLookup
import io.github.kuddev.pebrel.mobile.connection.PairingCodeRejected
import io.github.kuddev.pebrel.mobile.connection.PairingDiscovery
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
internal fun ShortCodePairing(onConnect: (String) -> Unit) {
    val context = LocalContext.current
    var attempt by remember { mutableIntStateOf(0) }
    val discovery = remember(attempt) { PairingDiscovery(context) }
    val state by discovery.state.collectAsState()
    var selected by remember { mutableStateOf<String?>(null) }
    var code by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var waited by remember(attempt) { mutableStateOf(false) }
    var error by remember { mutableStateOf<Int?>(null) }
    val scope = rememberCoroutineScope()
    var request by remember { mutableStateOf<Job?>(null) }
    val connect by rememberUpdatedState(onConnect)
    DisposableEffect(discovery) {
        discovery.start()
        onDispose { discovery.close() }
    }
    LaunchedEffect(discovery) { delay(8_000); waited = true }
    LaunchedEffect(state.computers) {
        if (state.computers.none { it.id == selected }) selected = state.computers.singleOrNull()?.id
    }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.fillMaxWidth().workspaceFrame().selectableGroup()) {
            for ((index, computer) in state.computers.withIndex()) {
                if (index > 0) HorizontalDivider(Modifier.padding(start = 16.dp), color = MaterialTheme.colorScheme.outlineVariant)
                Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
                    .selectable(selected == computer.id, enabled = !busy, role = Role.RadioButton,
                        onClick = { selected = computer.id; error = null })
                    .padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    RadioButton(selected == computer.id, onClick = null, enabled = !busy)
                    Column(Modifier.weight(1f).padding(start = 10.dp)) {
                        Text(computer.name, fontSize = 15.sp, lineHeight = 21.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text("${computer.address.host} · ${stringResource(R.string.pair_lan)}", fontSize = 12.sp, lineHeight = 18.sp,
                            fontFamily = LocalTerminalFont.current, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            if (state.computers.isEmpty()) {
                HelperText(stringResource(if (state.failed || waited) R.string.pair_discovery_empty else R.string.pair_discovery_searching),
                    Modifier.padding(16.dp))
            }
        }
        HelperText(stringResource(R.string.pair_code_network_hint), Modifier.padding(horizontal = 2.dp))
        if (!busy) TextButton({ attempt++; selected = null; error = null },
            modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.pair_discovery_retry)) }
        PairingCodeField(code, { code = it; error = null }, enabled = !busy, isError = error == R.string.pair_code_rejected)
        Text(stringResource(R.string.pair_code_digits_hint), fontSize = 12.sp, lineHeight = 20.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.align(Alignment.CenterHorizontally))
        error?.let { Text(stringResource(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
        val selectedComputer = state.computers.firstOrNull { it.id == selected }
        val connectLabel = when {
            busy -> stringResource(R.string.pair_code_connecting)
            selectedComputer != null -> stringResource(R.string.pair_code_connect_computer, selectedComputer.name)
            else -> stringResource(R.string.pair_code_connect)
        }
        ConnectionButton(connectLabel,
            enabled = !busy && code.length == 8 && state.computers.any { it.id == selected }) {
            val computer = state.computers.firstOrNull { it.id == selected } ?: return@ConnectionButton
            busy = true
            error = null
            request = scope.launch {
                try { connect(PairingCodeLookup.redeem(computer, code).toJson().toString()) }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (_: PairingCodeRejected) { error = R.string.pair_code_rejected }
                catch (_: Exception) { error = R.string.pair_code_failed }
                finally { busy = false }
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            if (busy) TextButton({ request?.cancel() }) { Text(stringResource(R.string.cancel)) }
        }
    }
}

@Composable
internal fun PairingCodeField(value: String, onChange: (String) -> Unit, enabled: Boolean = true, isError: Boolean = false) {
    val interactions = remember { MutableInteractionSource() }
    val focused by interactions.collectIsFocusedAsState()
    val colors = MaterialTheme.colorScheme
    val label = stringResource(R.string.pair_short_code)
    BasicTextField(value, { onChange(it.filter { digit -> digit in '0'..'9' }.take(8)) }, enabled = enabled,
        singleLine = true, interactionSource = interactions,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).semantics { contentDescription = label },
        decorationBox = { field ->
            Box {
                // 保留一个真实输入框负责粘贴、选择和无障碍，不创建八个互相抢焦点的字段。
                Box(Modifier.matchParentSize().alpha(0f)) { field() }
                Row(Modifier.fillMaxWidth().clearAndSetSemantics { }, verticalAlignment = Alignment.CenterVertically) {
                    repeat(8) { index ->
                        if (index > 0) Spacer(Modifier.width(if (index == 4) 12.dp else 6.dp))
                        val active = focused && index == value.length.coerceAtMost(7)
                        Box(Modifier.weight(1f).heightIn(min = 46.dp)
                            .border(1.dp, when { isError -> colors.error; active -> colors.primary; else -> colors.outlineVariant }, RoundedCornerShape(10.dp))
                            .padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
                            Text(value.getOrNull(index)?.toString().orEmpty(), fontSize = 19.sp, lineHeight = 26.sp,
                                fontFamily = LocalTerminalFont.current, color = if (enabled) colors.onSurface else colors.onSurfaceVariant)
                        }
                    }
                }
            }
        })
}
