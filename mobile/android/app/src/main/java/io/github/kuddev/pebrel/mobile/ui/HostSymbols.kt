package io.github.kuddev.pebrel.mobile.ui

import android.graphics.Paint
import android.graphics.Rect
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.HostIconSpec
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.R

@Composable
private fun iconLabel(icon: HostIconSpec) =
    if (LocalConfiguration.current.locales[0].language == "zh") icon.zh else icon.en

@Composable
fun HostSymbol(id: String, modifier: Modifier = Modifier) {
    val app = LocalContext.current.applicationContext as PebrelApplication
    val icon = app.hostIcons.find { it.id == id } ?: app.hostIcons.last { it.id == "term" }
    val paint = remember { Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = app.terminalTypeface } }
    val bounds = remember { Rect() }
    val color = MaterialTheme.colorScheme.onSurfaceVariant.toArgb()
    Canvas(modifier.size(24.dp)) {
        paint.color = color
        paint.textSize = size.height
        paint.getTextBounds(icon.glyph, 0, icon.glyph.length, bounds)
        val scale = minOf(size.width / bounds.width().coerceAtLeast(1), size.height / bounds.height().coerceAtLeast(1)) * .85f
        paint.textSize *= scale
        paint.getTextBounds(icon.glyph, 0, icon.glyph.length, bounds)
        drawIntoCanvas { it.nativeCanvas.drawText(icon.glyph,
            (size.width - bounds.width()) / 2 - bounds.left,
            (size.height - bounds.height()) / 2 - bounds.top, paint) }
    }
}

@Composable
fun HostIconChoice(id: String, onSelect: (String) -> Unit) {
    val app = LocalContext.current.applicationContext as PebrelApplication
    val icon = app.hostIcons.find { it.id == id } ?: app.hostIcons.last { it.id == "term" }
    var expanded by remember { mutableStateOf(false) }
    Box {
        OutlinedButton({ expanded = true }, shape = MaterialTheme.shapes.small, contentPadding = PaddingValues(horizontal = 12.dp),
            modifier = Modifier.heightIn(min = 48.dp)) {
            HostSymbol(icon.id)
            Text(iconLabel(icon), fontSize = 12.sp, modifier = Modifier.padding(horizontal = 9.dp))
            Glyph(R.drawable.ic_down, Modifier.size(13.dp))
        }
        DropdownMenu(expanded, { expanded = false }, Modifier.heightIn(max = 330.dp)) {
            app.hostIcons.forEach { entry ->
                DropdownMenuItem(text = { Text(iconLabel(entry), fontSize = 13.sp) },
                    leadingIcon = { HostSymbol(entry.id) }, onClick = { onSelect(entry.id); expanded = false })
            }
        }
    }
}
