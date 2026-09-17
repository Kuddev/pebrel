package io.github.kuddev.pebrel.mobile.ui

import android.animation.ValueAnimator
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.ExperimentalAnimationApi
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.core.FastOutLinearInEasing
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.unit.IntSize

enum class PebrelNavigationDirection {
    Forward,
    Backward,
}

/**
 * Small, shared motion vocabulary for the native shell.
 *
 * Compose applies the platform motion duration scale to these specs. The
 * Android animator switch is checked separately so an accessibility setting of
 * zero returns the Compose no-op variant instead of leaving a partial layout.
 */
@Immutable
data class PebrelMotion(
    val animationsEnabled: Boolean,
) {
    @OptIn(ExperimentalAnimationApi::class)
    fun pageTransition(direction: PebrelNavigationDirection): ContentTransform {
        if (!animationsEnabled) return ContentTransform(EnterTransition.None, ExitTransition.None, sizeTransform = null)
        val forward = direction == PebrelNavigationDirection.Forward
        val enterOffset: (Int) -> Int = { width -> if (forward) width / 6 else -width / 6 }
        val exitOffset: (Int) -> Int = { width -> if (forward) -width / 6 else width / 6 }
        val enter = fadeIn(
            animationSpec = tween(220, easing = FastOutSlowInEasing),
        ) + slideInHorizontally(
            animationSpec = tween(260, easing = FastOutSlowInEasing),
            initialOffsetX = enterOffset,
        )
        val exit = fadeOut(
            animationSpec = tween(170, easing = FastOutLinearInEasing),
        ) + slideOutHorizontally(
            animationSpec = tween(210, easing = FastOutLinearInEasing),
            targetOffsetX = exitOffset,
        )
        return ContentTransform(
            targetContentEnter = enter,
            initialContentExit = exit,
            sizeTransform = SizeTransform(clip = false),
        )
    }

    @OptIn(ExperimentalAnimationApi::class)
    fun collectionTransition(): ContentTransform {
        if (!animationsEnabled) return ContentTransform(EnterTransition.None, ExitTransition.None, sizeTransform = null)
        val enter = fadeIn(
            animationSpec = tween(180, easing = FastOutSlowInEasing),
        ) + expandVertically(
            animationSpec = tween(230, easing = FastOutSlowInEasing),
        )
        val exit = fadeOut(
            animationSpec = tween(130, easing = FastOutLinearInEasing),
        ) + shrinkVertically(
            animationSpec = tween(190, easing = FastOutLinearInEasing),
        )
        return ContentTransform(
            targetContentEnter = enter,
            initialContentExit = exit,
            sizeTransform = SizeTransform(clip = false),
        )
    }

    fun <T> tweenOrSnap(durationMillis: Int): FiniteAnimationSpec<T> =
        if (animationsEnabled) tween(durationMillis) else snap()

    fun contentSizeSpec(): FiniteAnimationSpec<IntSize> {
        if (!animationsEnabled) return snap()
        return spring(
            dampingRatio = Spring.DampingRatioNoBouncy,
            stiffness = Spring.StiffnessMediumLow,
        )
    }
}

@Composable
fun rememberPebrelMotion(): PebrelMotion {
    val enabled = ValueAnimator.areAnimatorsEnabled()
    return remember(enabled) { PebrelMotion(enabled) }
}
