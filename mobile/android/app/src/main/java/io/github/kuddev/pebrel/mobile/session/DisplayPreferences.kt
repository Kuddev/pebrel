package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Stable values shared by the settings page and the native terminal surface. */
object TerminalPreferenceValues {
    const val MAPLE_FONT = "maple"
    const val JETBRAINS_FONT = "jetbrains"
    const val SYSTEM_FONT = "system"
    const val BAR_CURSOR = "bar"
    const val UNDERLINE_CURSOR = "underline"
    const val BLOCK_CURSOR = "block"
    const val MIN_FONT_SIZE = 8
    const val MAX_FONT_SIZE = 32
}

data class TerminalPreferences(
    val fontFamily: String = TerminalPreferenceValues.MAPLE_FONT,
    val fontSize: Int = 16,
    val cursorStyle: String = TerminalPreferenceValues.BLOCK_CURSOR,
    val cursorBlink: Boolean = true,
    val pinchZoom: Boolean = true,
    val suggestions: Boolean = true,
    val directInput: Boolean = true,
) {
    /** Preserve the persisted contract when callers pass a stale or unknown value. */
    fun normalized(): TerminalPreferences = copy(
        fontFamily = when (fontFamily) {
            TerminalPreferenceValues.JETBRAINS_FONT -> TerminalPreferenceValues.JETBRAINS_FONT
            TerminalPreferenceValues.SYSTEM_FONT -> TerminalPreferenceValues.SYSTEM_FONT
            else -> TerminalPreferenceValues.MAPLE_FONT
        },
        fontSize = fontSize.coerceIn(TerminalPreferenceValues.MIN_FONT_SIZE, TerminalPreferenceValues.MAX_FONT_SIZE),
        cursorStyle = when (cursorStyle) {
            TerminalPreferenceValues.BAR_CURSOR -> TerminalPreferenceValues.BAR_CURSOR
            TerminalPreferenceValues.UNDERLINE_CURSOR -> TerminalPreferenceValues.UNDERLINE_CURSOR
            else -> TerminalPreferenceValues.BLOCK_CURSOR
        },
    )

    /** Compatibility constructor for the original three scalar preferences. */
    constructor(fontSize: Int, suggestions: Boolean, directInput: Boolean) : this(
        fontFamily = TerminalPreferenceValues.MAPLE_FONT,
        fontSize = fontSize,
        suggestions = suggestions,
        directInput = directInput,
    )
}

/** Small UI preferences only. Session identity, credentials and terminal state remain elsewhere. */
class DisplayPreferences(context: Context) {
    private val stored = context.getSharedPreferences("terminal_display", Context.MODE_PRIVATE)
    private val current = MutableStateFlow(
        TerminalPreferences(
            fontFamily = stored.getString("font_family", TerminalPreferenceValues.MAPLE_FONT).orEmpty(),
            fontSize = stored.getInt("font_size", 16),
            cursorStyle = stored.getString("cursor_style", TerminalPreferenceValues.BLOCK_CURSOR).orEmpty(),
            cursorBlink = stored.getBoolean("cursor_blink", true),
            pinchZoom = stored.getBoolean("pinch_zoom", true),
            suggestions = stored.getBoolean("suggestions", true),
            directInput = stored.getBoolean("direct_input", true),
        ).normalized(),
    )
    val state = current.asStateFlow()

    fun update(value: TerminalPreferences) {
        current.value = value.normalized()
        stored.edit().putString("font_family", current.value.fontFamily)
            .putInt("font_size", current.value.fontSize)
            .putString("cursor_style", current.value.cursorStyle)
            .putBoolean("cursor_blink", current.value.cursorBlink)
            .putBoolean("pinch_zoom", current.value.pinchZoom)
            .putBoolean("suggestions", current.value.suggestions)
            .putBoolean("direct_input", current.value.directInput).apply()
    }

    fun update(transform: (TerminalPreferences) -> TerminalPreferences) {
        update(transform(current.value))
    }
}
