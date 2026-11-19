package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.lerp
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.PebrelApplication
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class GlassTokensTest {
    @Test fun anyVetoKeepsFloatingSurfacesTranslucent() {
        val capable = GlassEnvironment(apiLevel = MIN_BLUR_API)
        assertEquals(GlassRendering.Blur, capable.rendering)
        listOf(capable.copy(apiLevel = MIN_BLUR_API - 1), capable.copy(powerSave = true), capable.copy(animatorsEnabled = false),
            capable.copy(windowBlursDisabled = true), capable.copy(lowRamDevice = true)).forEach { vetoed ->
            assertEquals(vetoed.toString(), GlassRendering.Translucent, vetoed.rendering)
        }
    }

    @Test fun everyRegisteredThemeKeepsTextReadableOnBothGlassPaths() {
        val themes = ApplicationProvider.getApplicationContext<PebrelApplication>().themes
        val names = themes.keys().asSequence().toList()
        assertTrue(names.size >= 16)
        for (name in names) {
            val palette = themes.getJSONObject(name)
            fun color(role: String) = palette.getJSONArray(role).let { Color(it.getInt(0), it.getInt(1), it.getInt(2)) }
            val background = color("background")
            val text = color("foreground")
            val muted = color("muted")
            val glass = deriveGlassPalette(background, text, muted)
            // Blurred text behind the surface diffuses toward the page.
            val diffused = lerp(background, text, .5f)
            for (tier in GlassTier.entries) {
                val blurred = glass.tint(tier).compositeOver(diffused)
                assertTrue("$name $tier body", contrastRatio(text, blurred) >= 4.5f)
                assertTrue("$name $tier secondary", contrastRatio(muted, blurred) >= 3f)
                // Without blur the strokes behind stay sharp; the solid fallback must still cover them.
                val solid = glass.solid(tier).compositeOver(text)
                assertTrue("$name $tier fallback", contrastRatio(text, solid) >= 4.5f)
            }
            assertTrue("$name tertiary", contrastRatio(glass.textTertiary, background) >= 3f)
        }
    }
}
