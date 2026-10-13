package io.github.kuddev.pebrel.mobile.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.*

internal class ReaderClipboard(private val context: Context, private val label: String,
                               private val scope: CoroutineScope, private val onFailure: () -> Unit) {
    var copied by mutableStateOf<Int?>(null)
        private set
    private var reset: Job? = null
    fun copy(text: String, index: Int) {
        try {
            context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText(label, text))
            copied = index
            reset?.cancel()
            reset = scope.launch { delay(1500); copied = null }
        } catch (_: Exception) { onFailure() }
    }
    fun close() { reset?.cancel() }
}

@Composable
internal fun rememberReaderClipboard(label: String, onFailure: () -> Unit): ReaderClipboard {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val failure = rememberUpdatedState(onFailure)
    val feedback = remember(context, label) { ReaderClipboard(context, label, scope) { failure.value() } }
    DisposableEffect(feedback) { onDispose { feedback.close() } }
    return feedback
}
