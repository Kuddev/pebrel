package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb

internal data class ReaderColors(val background: String, val foreground: String, val muted: String,
                                 val surface: String, val border: String, val accent: String,
                                 val codeSurface: String = surface)

internal fun ColorScheme.readerColors() = ReaderColors(
    background.readerHex(), onBackground.readerHex(), onSurfaceVariant.readerHex(),
    surfaceContainer.readerHex(), outlineVariant.readerHex(), primary.readerHex(),
    // 正文容器在 Nord 等主题里等于背景；代码使用独立、已定义的抬高色面。
    codeSurface = surfaceContainerHighest.readerHex(),
)

internal fun Color.readerHex(): String = "#%06x".format(toArgb() and 0xffffff)
