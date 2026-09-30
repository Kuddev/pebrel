package io.github.kuddev.pebrel.mobile.ui

import android.content.Intent
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import org.json.JSONObject

@Composable
fun SettingsScreen(repository: SessionRepository, onBack: () -> Unit, onComputers: () -> Unit,
                   onBackground: (Boolean) -> Unit, initial: String = "") {
    var page by rememberSaveable { mutableStateOf(initial) }
    var direction by remember { mutableStateOf(PebrelNavigationDirection.Forward) }
    val prefs by repository.display.state.collectAsStateWithLifecycle()
    val theme by repository.theme.collectAsStateWithLifecycle()
    val background by repository.backgroundActive.collectAsStateWithLifecycle()
    val sessions by repository.sessions.collectAsStateWithLifecycle()
    val desktops by repository.desktops.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val motion = rememberPebrelMotion()
    fun open(destination: String) { direction = PebrelNavigationDirection.Forward; page = destination }
    fun back() {
        if (page.isEmpty() || initial.isNotEmpty()) onBack()
        else { direction = PebrelNavigationDirection.Backward; page = "" }
    }
    BackHandler { back() }
    AnimatedContent(page, modifier = Modifier.fillMaxSize(),
        transitionSpec = { motion.pageTransition(direction) }, label = "settings_page") { route ->
        val title = when (route) {
            "theme" -> R.string.theme_settings
            "font" -> R.string.terminal_font_and_size
            "cursor" -> R.string.terminal_cursor
            "gestures" -> R.string.terminal_gestures
            "input" -> R.string.input_settings
            "completion" -> R.string.completion_settings
            "keepalive" -> R.string.background_title
            "notices" -> R.string.notifications
            "about" -> R.string.about
            else -> R.string.settings
        }
        Column(Modifier.fillMaxSize()) {
            PageHeader(stringResource(title), ::back)
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())
                .padding(horizontal = 22.dp, vertical = 8.dp)) {
                when (route) {
                    "" -> {
                        SettingsGroup(stringResource(R.string.raw_terminal)) {
                            SettingsRow(R.drawable.ic_palette, stringResource(R.string.theme_settings),
                                if (theme == "system") stringResource(R.string.follow_system) else theme) { open("theme") }
                            SettingDivider()
                            SettingsRow(R.drawable.ic_font, stringResource(R.string.terminal_font_and_size),
                                "${terminalFontLabel(prefs.fontFamily)} · ${prefs.fontSize}") { open("font") }
                            SettingDivider()
                            SettingsRow(R.drawable.ic_terminal, stringResource(R.string.terminal_cursor),
                                cursorLabel(prefs.cursorStyle)) { open("cursor") }
                        }
                        SettingsGroup(stringResource(R.string.input_group)) {
                            SettingsRow(R.drawable.ic_keyboard, stringResource(R.string.input_settings),
                                stringResource(if (prefs.directInput) R.string.direct_input else R.string.local_compose)) { open("input") }
                            SettingDivider()
                            SettingsRow(R.drawable.ic_touch, stringResource(R.string.terminal_gestures)) { open("gestures") }
                            SettingDivider()
                            SettingsRow(R.drawable.ic_command, stringResource(R.string.completion_settings),
                                enabledLabel(prefs.suggestions)) { open("completion") }
                        }
                        SettingsGroup(stringResource(R.string.connection_group)) {
                            SettingsRow(R.drawable.ic_monitor, stringResource(R.string.computers), onClick = onComputers)
                        }
                        SettingsGroup(stringResource(R.string.background_title)) {
                            SettingsRow(R.drawable.ic_battery, stringResource(R.string.keep_background),
                                enabledLabel(background)) { open("keepalive") }
                        }
                        SettingsGroup(stringResource(R.string.app_group)) {
                            SettingsRow(R.drawable.ic_bell, stringResource(R.string.notifications)) { open("notices") }
                            SettingDivider()
                            SettingsRow(R.drawable.ic_info, stringResource(R.string.about)) { open("about") }
                        }
                        Row(Modifier.fillMaxWidth().padding(vertical = 18.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                            HelperText("PEBREL")
                            HelperText(stringResource(R.string.preview_version))
                        }
                    }
                    "theme" -> ThemeChoices(repository)
                    "font" -> {
                        PreviewPanel(prefs.fontSize)
                        SettingsGroup(stringResource(R.string.font_settings)) {
                            listOf("maple", "jetbrains", "system").forEachIndexed { index, family ->
                                if (index > 0) SettingDivider()
                                SettingsChoice(terminalFontLabel(family), prefs.fontFamily == family) {
                                    repository.display.update { it.copy(fontFamily = family) }
                                }
                            }
                        }
                        SettingsGroup(stringResource(R.string.font_size)) {
                            Column(Modifier.padding(horizontal = 18.dp, vertical = 12.dp)) {
                                Text(stringResource(R.string.terminal_font_size_value, prefs.fontSize), fontSize = 14.sp)
                                Slider(prefs.fontSize.toFloat(), { value -> repository.display.update { it.copy(fontSize = value.toInt()) } },
                                    valueRange = 8f..32f, steps = 23)
                            }
                        }
                        HelperText(stringResource(R.string.font_boundary))
                    }
                    "cursor" -> {
                        PreviewPanel(prefs.fontSize)
                        SettingsGroup(stringResource(R.string.terminal_cursor_shape)) {
                            listOf("block", "bar", "underline").forEachIndexed { index, style ->
                                if (index > 0) SettingDivider()
                                SettingsChoice(cursorLabel(style), prefs.cursorStyle == style) {
                                    repository.display.update { it.copy(cursorStyle = style) }
                                }
                            }
                            SettingDivider()
                            TogglePreference(stringResource(R.string.terminal_cursor_blink), prefs.cursorBlink) {
                                repository.display.update { value -> value.copy(cursorBlink = it) }
                            }
                        }
                        HelperText(stringResource(R.string.terminal_cursor_hint))
                    }
                    "gestures" -> {
                        SettingsGroup(stringResource(R.string.terminal_gestures)) {
                            TogglePreference(stringResource(R.string.terminal_pinch_zoom), prefs.pinchZoom) {
                                repository.display.update { value -> value.copy(pinchZoom = it) }
                            }
                        }
                        HelperText(stringResource(R.string.terminal_pinch_hint))
                    }
                    "input" -> {
                        SettingsGroup(stringResource(R.string.input_settings)) {
                            listOf(false, true).forEachIndexed { index, direct ->
                                if (index > 0) SettingDivider()
                                SettingsChoice(stringResource(if (direct) R.string.direct_input else R.string.local_compose), prefs.directInput == direct) {
                                    repository.display.update { it.copy(directInput = direct) }
                                }
                            }
                        }
                        HelperText(stringResource(R.string.input_settings_hint))
                    }
                    "completion" -> {
                        SettingsGroup(stringResource(R.string.input_group)) {
                            TogglePreference(stringResource(R.string.completion_settings), prefs.suggestions) {
                                repository.display.update { value -> value.copy(suggestions = it) }
                            }
                        }
                        HelperText(stringResource(R.string.completion_boundary))
                    }
                    "keepalive" -> {
                        val available = background || sessions.any { it.status in setOf("ready", "connecting") } ||
                            desktops.any { it.status in setOf("ready", "connecting") }
                        SettingsGroup(stringResource(R.string.background_title)) {
                            TogglePreference(stringResource(R.string.keep_background), background, available, onBackground)
                        }
                        HelperText(stringResource(R.string.background_boundary))
                    }
                    "notices" -> {
                        SettingsGroup(stringResource(R.string.notifications)) {
                            SettingsRow(R.drawable.ic_bell, stringResource(R.string.notification_settings)) {
                                context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName))
                            }
                        }
                        HelperText(stringResource(R.string.notice_boundary))
                    }
                    "about" -> {
                        Text("Pebrel", fontSize = 24.sp, modifier = Modifier.padding(vertical = 20.dp))
                        HelperText(stringResource(R.string.ui_preview_engine))
                        HelperText(stringResource(R.string.connection_boundary), Modifier.padding(vertical = 20.dp))
                        HelperText(stringResource(R.string.features_pending))
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

@Composable
private fun enabledLabel(enabled: Boolean) = stringResource(if (enabled) R.string.enabled else R.string.disabled)

@Composable
private fun terminalFontLabel(family: String): String = when (family) {
    "jetbrains" -> "JetBrains Mono"
    "system" -> stringResource(R.string.terminal_system_font)
    else -> "Maple Mono NF CN"
}

@Composable
private fun cursorLabel(style: String) = stringResource(when (style) {
    "bar" -> R.string.terminal_cursor_bar
    "underline" -> R.string.terminal_cursor_underline
    else -> R.string.terminal_cursor_block
})

@Composable
private fun PreviewPanel(fontSize: Int = 16) {
    val shape = RoundedCornerShape(14.dp)
    Column(Modifier.padding(top = 16.dp).fillMaxWidth().clip(shape)
        .background(MaterialTheme.colorScheme.background).border(.5.dp, MaterialTheme.colorScheme.outlineVariant, shape).padding(18.dp)) {
        HelperText(stringResource(R.string.theme_preview))
        Text(stringResource(R.string.font_preview), fontSize = fontSize.sp, fontFamily = LocalTerminalFont.current,
            modifier = Modifier.padding(top = 18.dp))
        HelperText(stringResource(R.string.terminal_solid_background), Modifier.padding(top = 14.dp))
    }
}

@Composable
private fun ThemeChoices(repository: SessionRepository) {
    val catalog = (LocalContext.current.applicationContext as PebrelApplication).themes
    val choice by repository.theme.collectAsStateWithLifecycle()
    fun JSONObject.swatch(role: String): Color = getJSONArray(role).let { Color(it.getInt(0), it.getInt(1), it.getInt(2)) }
    PreviewPanel()
    SettingsGroup(stringResource(R.string.theme_settings)) {
        SettingsChoice(stringResource(R.string.follow_system), choice == "system") { repository.selectTheme("system") }
    }
    listOf(false, true).forEach { dark ->
        val names = catalog.keys().asSequence().toList().filter {
            (catalog.getJSONObject(it).swatch("background").luminance() < .5f) == dark
        }
        SettingsGroup(stringResource(if (dark) R.string.dark_themes else R.string.light_themes)) {
            names.forEachIndexed { index, name ->
                if (index > 0) SettingDivider()
                Row(Modifier.fillMaxWidth().clickable { repository.selectTheme(name) }.heightIn(min = 58.dp).padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    Text(name, fontSize = 14.sp, modifier = Modifier.weight(1f))
                    listOf("background", "foreground", "red", "green", "blue").forEach { role ->
                        Box(Modifier.size(12.dp).background(catalog.getJSONObject(name).swatch(role), RoundedCornerShape(2.dp)))
                    }
                    Box(Modifier.padding(start = 8.dp).size(20.dp)) { if (name == choice) Glyph(R.drawable.ic_check) }
                }
            }
        }
    }
}
