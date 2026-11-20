package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import io.github.kuddev.pebrel.terminal.TerminalFrame

/** Only the active desktop pane adopts remote colors. Settings and SSH keep
 * the user's phone theme. No persistent theme preference is changed. */
internal fun desktopTerminalColors(frame: TerminalFrame?, fallback: ColorScheme): ColorScheme {
    if (frame == null || frame.meta.size < 10) return fallback
    val background = Color(frame.background)
    val foreground = Color(frame.meta[8])
    val accent = Color(frame.meta[6])
    val base = if (background.luminance() < .5f) darkColorScheme() else lightColorScheme()
    val surface = lerp(background, foreground, .06f)
    return base.copy(
        background = background, surface = background,
        onBackground = foreground, onSurface = foreground,
        primary = accent, onPrimary = background,
        onSurfaceVariant = lerp(background, foreground, .72f),
        surfaceVariant = surface, surfaceContainer = surface,
        surfaceContainerLow = background, surfaceContainerHigh = surface,
        secondaryContainer = surface, onSecondaryContainer = foreground,
        outline = lerp(background, foreground, .4f),
        outlineVariant = lerp(background, foreground, .2f),
        error = Color(frame.meta[9]), surfaceTint = Color.Transparent,
    )
}

@Composable
fun DesktopTerminalTheme(frame: TerminalFrame?, systemBars: Boolean = false, content: @Composable () -> Unit) {
    val colors = desktopTerminalColors(frame, MaterialTheme.colorScheme)
    val view = LocalView.current
    if (systemBars) SideEffect {
        (view.context as? android.app.Activity)?.window?.let { window ->
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = colors.background.luminance() >= .5f
                isAppearanceLightNavigationBars = colors.background.luminance() >= .5f
            }
        }
    }
    MaterialTheme(colorScheme = colors, content = content)
}
