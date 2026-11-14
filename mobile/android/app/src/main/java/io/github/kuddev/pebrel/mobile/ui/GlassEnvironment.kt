package io.github.kuddev.pebrel.mobile.ui

import android.app.ActivityManager
import android.content.BroadcastReceiver
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.ContentObserver
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat

/** How floating surfaces are drawn on this device right now. */
internal enum class GlassRendering { Blur, Translucent }

/** Everything that may veto a live backdrop blur. Kept free of Android calls so the rule is testable. */
internal data class GlassEnvironment(
    val apiLevel: Int,
    val powerSave: Boolean = false,
    val animatorsEnabled: Boolean = true,
    val windowBlursDisabled: Boolean = false,
    val lowRamDevice: Boolean = false,
) {
    val rendering: GlassRendering
        get() = if (apiLevel >= MIN_BLUR_API && !powerSave && animatorsEnabled && !windowBlursDisabled && !lowRamDevice)
            GlassRendering.Blur else GlassRendering.Translucent
}

/** RenderEffect is the first blur that does not read pixels back on the UI thread. */
internal const val MIN_BLUR_API = 31

/** Translucent is the safe default: previews and tests must not record backdrops. */
internal val LocalGlassRendering = compositionLocalOf { GlassRendering.Translucent }

// The developer option behind `wm disable-blur`; hidden, so its absence must not break the app.
private const val DISABLE_WINDOW_BLURS = "disable_window_blurs"

private fun windowBlursDisabled(resolver: ContentResolver): Boolean =
    runCatching { Settings.Global.getInt(resolver, DISABLE_WINDOW_BLURS, 0) != 0 }.getOrDefault(false)

internal fun readGlassEnvironment(context: Context): GlassEnvironment {
    val resolver = context.contentResolver
    return GlassEnvironment(
        apiLevel = Build.VERSION.SDK_INT,
        powerSave = context.getSystemService(PowerManager::class.java)?.isPowerSaveMode == true,
        // The setting itself, not ValueAnimator's copy, which the system updates after observers run.
        animatorsEnabled = Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) != 0f,
        windowBlursDisabled = windowBlursDisabled(resolver),
        lowRamDevice = context.getSystemService(ActivityManager::class.java)?.isLowRamDevice == true,
    )
}

/** Battery saver and reduced motion change while the app runs; follow them without a restart. */
@Composable
internal fun rememberGlassRendering(): GlassRendering {
    val context = LocalContext.current.applicationContext
    var environment by remember(context) { mutableStateOf(readGlassEnvironment(context)) }
    DisposableEffect(context) {
        // Below API 31 no setting can enable blur, so there is nothing to follow.
        if (Build.VERSION.SDK_INT < MIN_BLUR_API) return@DisposableEffect onDispose {}
        val refresh = { environment = readGlassEnvironment(context) }
        val power = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) { refresh() }
        }
        val settings = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) { refresh() }
        }
        ContextCompat.registerReceiver(context, power, IntentFilter(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED),
            ContextCompat.RECEIVER_NOT_EXPORTED)
        val resolver = context.contentResolver
        resolver.registerContentObserver(Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE), false, settings)
        resolver.registerContentObserver(Settings.Global.getUriFor(DISABLE_WINDOW_BLURS), false, settings)
        refresh()
        onDispose {
            context.unregisterReceiver(power)
            resolver.unregisterContentObserver(settings)
        }
    }
    return environment.rendering
}
