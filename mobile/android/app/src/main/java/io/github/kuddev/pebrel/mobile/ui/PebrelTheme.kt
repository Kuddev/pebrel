package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.core.view.WindowCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import org.json.JSONObject

val LocalTerminalFont = staticCompositionLocalOf<FontFamily> { FontFamily.Monospace }

private fun JSONObject.color(key: String): Color {
    val value = getJSONArray(key)
    return Color(value.getInt(0), value.getInt(1), value.getInt(2), if (value.length() == 4) value.getInt(3) else 255)
}

@Composable
fun PebrelTheme(content: @Composable () -> Unit) {
    val application = LocalContext.current.applicationContext as PebrelApplication
    val catalog = application.themes
    val preferences by application.sessions.display.state.collectAsStateWithLifecycle()
    val terminalFont = remember(application, preferences.fontFamily) {
        FontFamily(application.terminalTypeface(preferences.fontFamily))
    }
    val choice by application.sessions.theme.collectAsStateWithLifecycle()
    val systemDark = isSystemInDarkTheme()
    val name = if (catalog.has(choice)) choice else if (systemDark) "Nord" else "Paper"
    val palette = remember(name) { catalog.getJSONObject(name) }
    val background = palette.color("background")
    val dark = background.luminance() < 0.5f
    val scheme = (if (dark) darkColorScheme() else lightColorScheme()).copy(
        primary = palette.color("accent"), onPrimary = background,
        tertiary = palette.color("green"), onTertiary = background,
        background = background, surface = palette.color("shell"),
        onBackground = palette.color("foreground"), onSurface = palette.color("foreground"),
        onSurfaceVariant = palette.color("muted"), outline = palette.color("frame"),
        outlineVariant = palette.color("line"), secondaryContainer = palette.color("selected"),
        surfaceVariant = palette.color("selected"),
        onSecondaryContainer = palette.color("foreground"), error = palette.color("red"),
        surfaceContainer = background, surfaceContainerLow = background,
        surfaceContainerHigh = palette.color("shell"), surfaceTint = Color.Transparent,
    )
    val view = LocalView.current
    SideEffect {
        (view.context as? android.app.Activity)?.window?.let { window ->
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }
    LaunchedEffect(name) {
        fun argb(role: String): Int {
            val rgb = palette.getJSONArray(role)
            return android.graphics.Color.rgb(rgb.getInt(0), rgb.getInt(1), rgb.getInt(2))
        }
        val roles = listOf("background", "red", "green", "yellow", "blue", "purple", "cyan", "foreground")
        application.sessions.setTerminalColors(intArrayOf(argb("foreground"), argb("background"), argb("accent")) +
            (roles + roles).map(::argb).toIntArray())
    }
    MaterialTheme(colorScheme = scheme, typography = Typography(
        bodyLarge = TextStyle(fontSize = 15.sp, lineHeight = 23.sp),
        bodyMedium = TextStyle(fontSize = 14.sp, lineHeight = 21.sp),
        bodySmall = TextStyle(fontSize = 12.sp, lineHeight = 19.sp),
        labelLarge = TextStyle(fontSize = 13.sp, lineHeight = 20.sp),
    ), shapes = Shapes(
        extraSmall = RoundedCornerShape(8.dp), small = RoundedCornerShape(14.dp),
        medium = RoundedCornerShape(18.dp), large = RoundedCornerShape(20.dp),
        extraLarge = RoundedCornerShape(24.dp),
    )) {
        CompositionLocalProvider(LocalTerminalFont provides terminalFont, content = content)
    }
}
