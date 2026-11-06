package io.github.kuddev.pebrel.mobile.ui

import android.graphics.Paint
import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SheetState
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.dismiss
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import dev.chrisbanes.haze.ExperimentalHazeApi
import io.github.kuddev.pebrel.mobile.R
import kotlinx.coroutines.launch
import dev.chrisbanes.haze.HazeEffectScope
import dev.chrisbanes.haze.HazeInputScale
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.HazeStyle
import dev.chrisbanes.haze.HazeTint
import dev.chrisbanes.haze.hazeEffect
import dev.chrisbanes.haze.hazeSource

/** The scrolling content behind this page's bars; null means nothing is recorded. */
internal val LocalGlassBackdrop = compositionLocalOf<HazeState?> { null }

/** The whole window behind sheets, which live in their own dialog window. */
internal val LocalGlassOverlay = compositionLocalOf<HazeState?> { null }

// One shared block: a new lambda per composition would re-run the effect node's update.
@OptIn(ExperimentalHazeApi::class)
private val glassEffectScope: HazeEffectScope.() -> Unit = { inputScale = HazeInputScale.Auto }

/** Records the window for sheets, which live in their own dialog window. */
internal fun Modifier.glassOverlaySource(state: HazeState?): Modifier = if (state == null) this else hazeSource(state)

@Composable
internal fun rememberGlassOverlay(): HazeState? {
    val rendering = LocalGlassRendering.current
    return remember(rendering) { if (rendering == GlassRendering.Blur) HazeState() else null }
}

/**
 * One floating surface: soft shadow, then blur and tint, then the edge highlight.
 * With nothing recorded behind it the same tint is drawn solid, at an alpha that
 * keeps text readable even over sharp content.
 */
@Composable
internal fun Modifier.glass(
    shape: Shape,
    tier: GlassTier = GlassTier.Heavy,
    lift: GlassLift? = GlassLift.Floating,
    highlight: Boolean = true,
    state: HazeState? = LocalGlassBackdrop.current,
    accent: Color? = null,
): Modifier {
    val palette = glassPalette()
    val backdrop = MaterialTheme.colorScheme.background
    // An accent surface stays recognisably the action color; only its body lets the blur through.
    val tint = accent?.copy(alpha = .88f) ?: palette.tint(tier)
    val solid = accent ?: palette.solid(tier)
    val style = remember(backdrop, tint, solid) {
        HazeStyle(backgroundColor = backdrop, tint = HazeTint(tint), blurRadius = GlassBlurRadius,
            noiseFactor = 0f, fallbackTint = HazeTint(solid))
    }
    val sampling = state?.takeIf { it.areas.isNotEmpty() }
    return this
        .then(if (lift != null) Modifier.softShadow(shape, palette, lift) else Modifier)
        .clip(shape)
        .then(if (sampling != null) Modifier.hazeEffect(sampling, style, glassEffectScope) else Modifier.background(solid))
        .then(if (highlight) Modifier.border(GlassHairline, palette.highlight, shape) else Modifier)
}

private fun shadowPaint(color: Color, radius: Float, offsetY: Float) = Paint(Paint.ANTI_ALIAS_FLAG).apply {
    // The shadow's alpha comes from its own color; the opaque fill is clipped away below.
    this.color = android.graphics.Color.BLACK
    setShadowLayer(radius, 0f, offsetY, color.toArgb())
}

/**
 * A translucent surface would show a shadow drawn under it, so the outline is clipped out.
 * Canvas shadows of arbitrary paths need API 28; earlier devices keep only the hairline.
 */
private fun Modifier.softShadow(shape: Shape, palette: GlassPalette, lift: GlassLift): Modifier {
    if (Build.VERSION.SDK_INT < 28) return this
    return drawWithCache {
        val outline = Path().apply { addOutline(shape.createOutline(size, layoutDirection, this@drawWithCache)) }
        val native = outline.asAndroidPath()
        val near = shadowPaint(palette.shadowNear, lift.nearBlur.toPx(), lift.nearY.toPx())
        val contact = shadowPaint(palette.shadowContact, lift.contactBlur.toPx(), lift.contactY.toPx())
        // A bar's shadow falls only onto the content below it, never upward into the system bar.
        val below = if (lift == GlassLift.Bar) Rect(0f, size.height, size.width, size.height + 3 * lift.nearBlur.toPx()) else null
        onDrawBehind {
            drawIntoCanvas { canvas ->
                canvas.save()
                if (below != null) canvas.clipRect(below) else canvas.clipPath(outline, ClipOp.Difference)
                canvas.nativeCanvas.drawPath(native, near)
                canvas.nativeCanvas.drawPath(native, contact)
                canvas.restore()
            }
        }
    }
}

/** A full-width bar: page-tinted glass with a bottom hairline, above whatever follows it in a column. */
@Composable
internal fun Modifier.glassBar(): Modifier {
    val hairline = glassPalette().hairline
    return zIndex(1f)
        .glass(RectangleShape, GlassTier.Bar, GlassLift.Bar, highlight = false)
        .drawWithContent {
            drawContent()
            val stroke = GlassHairline.toPx()
            drawRect(hairline, Offset(0f, size.height - stroke), Size(size.width, stroke))
        }
}

/**
 * Bars float above the content, which receives their heights as padding and scrolls under
 * them. [content] applies the source modifier to its scroll container. Its own children see
 * no backdrop: a glass effect drawn inside the source it samples would record itself.
 */
@Composable
internal fun GlassPage(
    modifier: Modifier = Modifier,
    topBar: @Composable () -> Unit = {},
    bottomBar: @Composable () -> Unit = {},
    floatingActionButton: @Composable () -> Unit = {},
    content: @Composable (padding: PaddingValues, source: Modifier) -> Unit,
) {
    val rendering = LocalGlassRendering.current
    val state = remember(rendering) { if (rendering == GlassRendering.Blur) HazeState() else null }
    val source = if (state == null) Modifier else Modifier.hazeSource(state)
    Scaffold(
        modifier,
        topBar = { CompositionLocalProvider(LocalGlassBackdrop provides state, content = topBar) },
        bottomBar = { CompositionLocalProvider(LocalGlassBackdrop provides state, content = bottomBar) },
        floatingActionButton = { CompositionLocalProvider(LocalGlassBackdrop provides state, content = floatingActionButton) },
        containerColor = Color.Transparent,
        contentColor = MaterialTheme.colorScheme.onBackground,
        contentWindowInsets = WindowInsets(0),
    ) { padding ->
        CompositionLocalProvider(LocalGlassBackdrop provides null) { content(padding, source) }
    }
}

/**
 * Material still owns the dialog window, dragging, focus and back handling. The container is
 * transparent so the handle and content sit on one glass surface sampling the app's window.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GlassSheet(onDismissRequest: () -> Unit, content: @Composable ColumnScope.() -> Unit) =
    GlassSheet(onDismissRequest, rememberModalBottomSheetState(), content)

/** For callers that hide the sheet themselves before acting, e.g. to open a form afterwards. */
@ExperimentalMaterial3Api
@Composable
internal fun GlassSheet(
    onDismissRequest: () -> Unit,
    sheetState: SheetState,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(topStart = 26.dp, topEnd = 26.dp)
    val scope = rememberCoroutineScope()
    val closeLabel = stringResource(R.string.close)
    ModalBottomSheet(
        onDismissRequest = onDismissRequest,
        sheetState = sheetState,
        shape = shape,
        containerColor = Color.Transparent,
        contentColor = colors.onSurface,
        scrimColor = colors.scrim.copy(alpha = .32f),
        dragHandle = null,
        contentWindowInsets = { WindowInsets(0) },
    ) {
        Column(Modifier.fillMaxWidth().glass(shape, GlassTier.Heavy, lift = null, state = LocalGlassOverlay.current)
            .navigationBarsPadding()) {
            // Material's own handle would sit outside this surface; keep its accessibility action here.
            Box(Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 8.dp).semantics {
                contentDescription = closeLabel
                dismiss {
                    scope.launch { sheetState.hide() }.invokeOnCompletion { onDismissRequest() }
                    true
                }
            }, contentAlignment = Alignment.Center) {
                Box(Modifier.size(32.dp, 4.dp).background(colors.onSurfaceVariant.copy(alpha = .4f), RoundedCornerShape(2.dp)))
            }
            content()
        }
    }
}

/** The session list: every local, SSH and computer session, plus entries to open a new one. */
@Composable
internal fun SessionSwitcherSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    GlassSheet(onDismiss) {
        Text(stringResource(R.string.all_sessions), style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(start = 22.dp, end = 22.dp, bottom = 6.dp).semantics { heading() })
        Column(Modifier.fillMaxWidth().heightIn(max = 560.dp).verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp).padding(bottom = 16.dp), content = content)
    }
}
