package io.github.kuddev.pebrel.mobile.ui

import android.view.inputmethod.InputMethodManager
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.viewinterop.AndroidView
import io.github.kuddev.pebrel.mobile.PebrelApplication
import io.github.kuddev.pebrel.mobile.session.LocalSession
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import io.github.kuddev.pebrel.mobile.session.TerminalPreferences
import io.github.kuddev.pebrel.terminal.GhosttyView

@Composable
fun TerminalSurface(session: LocalSession, repository: SessionRepository, modifier: Modifier, direct: Boolean, fontSize: Int, keyboardRequest: Int = 0) {
    val stored = repository.display.state.value
    TerminalSurface(
        session = session,
        repository = repository,
        modifier = modifier,
        preferences = stored.copy(fontSize = fontSize, directInput = direct),
        keyboardRequest = keyboardRequest,
        onPreferencesChanged = { value ->
            repository.display.update { current -> current.copy(fontSize = value.fontSize) }
        },
    )
}

@Composable
fun TerminalSurface(
    session: LocalSession,
    repository: SessionRepository,
    modifier: Modifier,
    preferences: TerminalPreferences,
    onPreferencesChanged: (TerminalPreferences) -> Unit = {},
    keyboardRequest: Int = 0,
) {
    val renderToken = remember(session.id) { Any() }
    val lastKeyboardRequest = remember(session.id) { mutableIntStateOf(keyboardRequest) }
    DisposableEffect(session.id, renderToken) { onDispose { repository.detachRenderer(session.id, renderToken) } }
    AndroidView(modifier = modifier.clipToBounds(), factory = { context -> GhosttyView(context) }, update = { view ->
        val app = view.context.applicationContext as PebrelApplication
        view.setTerminalPreferences(
            typeface = app.terminalTypeface(preferences.fontFamily),
            fontSize = preferences.fontSize,
            cursorStyle = preferences.cursorStyle,
            cursorBlink = preferences.cursorBlink,
            pinchZoom = preferences.pinchZoom,
            onFontSizeChanged = { size ->
                if (size != preferences.fontSize) onPreferencesChanged(preferences.copy(fontSize = size))
            },
        )
        view.directInput = preferences.directInput
        view.session = session.terminal
        repository.attachRenderer(session.id, renderToken) { view.onScreenUpdated() }
        if (keyboardRequest != lastKeyboardRequest.intValue) {
            lastKeyboardRequest.intValue = keyboardRequest
            if (preferences.directInput) {
                view.requestFocus()
                view.post {
                    if (view.isAttachedToWindow && view.directInput) {
                        view.context.getSystemService(InputMethodManager::class.java).showSoftInput(view, 0)
                    }
                }
            }
        }
    })
}
