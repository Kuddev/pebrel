package io.github.kuddev.pebrel.mobile.ui

import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R
import kotlinx.coroutines.delay

/** 字号反馈属于当前视图；不写终端输入，也不替渲染器持久化设置。 */
internal class TerminalZoomFeedback(private val view: View) {
    var size by mutableIntStateOf(0)
        private set
    var visible by mutableStateOf(false)
        private set
    var gesturing by mutableStateOf(false)
        private set

    fun update(size: Int, gesturing: Boolean) {
        // 只在跨过字号档位时轻触一次；遵守系统触感开关，不逐帧启动振动器。
        if (gesturing && this.gesturing && this.size != size) {
            view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
        }
        this.size = size
        this.gesturing = gesturing
        visible = true
    }

    fun dismiss() { visible = false }
}

@Composable
internal fun rememberTerminalZoomFeedback(identity: Any): TerminalZoomFeedback {
    val view = LocalView.current
    val feedback = remember(identity, view) { TerminalZoomFeedback(view) }
    LaunchedEffect(feedback, feedback.size, feedback.gesturing, feedback.visible) {
        if (feedback.visible && !feedback.gesturing) {
            delay(650)
            feedback.dismiss()
        }
    }
    return feedback
}

@Composable
internal fun BoxScope.TerminalZoomIndicator(feedback: TerminalZoomFeedback) {
    val motion = rememberPebrelMotion()
    // 仅覆盖绘制，不占正文布局，也不拦截双指或终端输入。
    AnimatedVisibility(feedback.visible, modifier = Modifier.align(Alignment.Center),
        enter = fadeIn(motion.tweenOrSnap(90)), exit = fadeOut(motion.tweenOrSnap(130))) {
        Box(Modifier.testTag("terminal-zoom-feedback")
            .background(MaterialTheme.colorScheme.inverseSurface, RoundedCornerShape(8.dp))
            .padding(horizontal = 16.dp, vertical = 10.dp)) {
            Text(stringResource(R.string.terminal_zoom_size, feedback.size),
                color = MaterialTheme.colorScheme.inverseOnSurface, fontSize = 14.sp)
        }
    }
}
