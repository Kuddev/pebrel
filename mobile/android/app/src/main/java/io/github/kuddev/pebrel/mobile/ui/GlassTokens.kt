package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// Wide enough that lines of text behind a surface melt together instead of showing as smudges.
internal val GlassBlurRadius = 40.dp
internal val GlassHairline = .5.dp

/**
 * Heavy carries text; light is for small controls that may show more of what is behind them.
 * Bar is tinted with the page itself, so a full-width bar never reads as a stripe of another color.
 */
internal enum class GlassTier { Heavy, Light, Bar }

/** Two offset soft shadows, large and diffuse plus small and close. */
internal enum class GlassLift(val nearBlur: Dp, val nearY: Dp, val contactBlur: Dp, val contactY: Dp) {
    Floating(16.dp, 8.dp, 4.dp, 1.dp),
    Raised(12.dp, 4.dp, 3.dp, 1.dp),
    Bar(8.dp, 2.dp, 2.dp, .5.dp),
}

/** Roles derived from a theme's palette. Android installs no extra themes for glass. */
@Immutable
internal data class GlassPalette(
    val dark: Boolean,
    val heavyTint: Color,
    val lightTint: Color,
    val heavySolid: Color,
    val lightSolid: Color,
    val barTint: Color,
    val barSolid: Color,
    val highlight: Color,
    val hairline: Color,
    val shadowNear: Color,
    val shadowContact: Color,
    val textTertiary: Color,
) {
    fun tint(tier: GlassTier) = when (tier) {
        GlassTier.Heavy -> heavyTint
        GlassTier.Light -> lightTint
        GlassTier.Bar -> barTint
    }

    fun solid(tier: GlassTier) = when (tier) {
        GlassTier.Heavy -> heavySolid
        GlassTier.Light -> lightSolid
        GlassTier.Bar -> barSolid
    }
}

private const val BODY_CONTRAST = 4.5f
private const val SECONDARY_CONTRAST = 3f
private const val SOLID_HEAVY_ALPHA = .94f
private const val SOLID_LIGHT_ALPHA = .88f

internal fun contrastRatio(first: Color, second: Color): Float {
    val a = first.compositeOver(Color.White).luminance()
    val b = second.compositeOver(Color.White).luminance()
    return (maxOf(a, b) + .05f) / (minOf(a, b) + .05f)
}

/** The lowest alpha, from [start], at which both text colors stay readable over [backdrop]. */
internal fun readableTintAlpha(tint: Color, text: Color, secondary: Color, backdrop: Color, start: Float): Float {
    var alpha = start
    while (alpha < 1f) {
        val surface = tint.copy(alpha = alpha).compositeOver(backdrop)
        if (contrastRatio(text, surface) >= BODY_CONTRAST && contrastRatio(secondary, surface) >= SECONDARY_CONTRAST) return alpha
        alpha += .02f
    }
    return 1f
}

internal fun deriveGlassPalette(background: Color, foreground: Color, muted: Color): GlassPalette {
    val opaque = background.compositeOver(Color.White)
    val dark = opaque.luminance() < .5f
    val tint = if (dark) lerp(opaque, foreground, .05f) else lerp(opaque, Color.White, .72f)
    // Text behind a blur diffuses toward the page; a middle mix is the realistic worst case.
    val diffused = lerp(opaque, foreground, .5f)
    val heavy = readableTintAlpha(tint, foreground, muted, diffused, if (dark) .84f else .82f)
    val light = readableTintAlpha(tint, foreground, muted, diffused, if (dark) .55f else .50f)
    // Without blur the strokes behind stay sharp, so the fallback has to cover them too.
    val solidHeavy = readableTintAlpha(tint, foreground, muted, foreground, SOLID_HEAVY_ALPHA)
    val solidLight = readableTintAlpha(tint, foreground, muted, foreground, SOLID_LIGHT_ALPHA)
    val bar = readableTintAlpha(opaque, foreground, muted, diffused, .84f)
    val solidBar = readableTintAlpha(opaque, foreground, muted, foreground, SOLID_HEAVY_ALPHA)
    return GlassPalette(
        dark = dark,
        heavyTint = tint.copy(alpha = heavy),
        lightTint = tint.copy(alpha = light),
        heavySolid = tint.copy(alpha = solidHeavy),
        lightSolid = tint.copy(alpha = solidLight),
        barTint = opaque.copy(alpha = bar),
        barSolid = opaque.copy(alpha = solidBar),
        highlight = Color.White.copy(alpha = if (dark) .06f else .60f),
        hairline = foreground.copy(alpha = if (dark) .16f else .12f),
        shadowNear = if (dark) Color.Black.copy(alpha = .30f) else foreground.copy(alpha = .07f),
        shadowContact = if (dark) Color.Black.copy(alpha = .20f) else foreground.copy(alpha = .04f),
        textTertiary = tertiaryText(muted, opaque),
    )
}

/** Quieter than secondary text, but never below the 3:1 floor for non-essential labels. */
internal fun tertiaryText(muted: Color, background: Color): Color {
    var mix = .35f
    while (mix > 0f) {
        val candidate = lerp(muted, background, mix)
        if (contrastRatio(candidate, background) >= SECONDARY_CONTRAST) return candidate
        mix -= .05f
    }
    return muted
}

@Composable
internal fun glassPalette(): GlassPalette {
    val colors = MaterialTheme.colorScheme
    return remember(colors.background, colors.onSurface, colors.onSurfaceVariant) {
        deriveGlassPalette(colors.background, colors.onSurface, colors.onSurfaceVariant)
    }
}
