package io.github.kuddev.pebrel.terminal

import android.text.Editable
import android.text.Selection
import android.view.KeyEvent
import android.view.View
import android.content.ClipboardManager
import android.view.inputmethod.BaseInputConnection

/** IME preedit stays local; only committed text enters the remote byte stream. */
internal class TerminalInputConnection(
    private val view: View,
    private val target: TerminalInputTarget,
    private val isCurrent: () -> Boolean,
    private val onComposing: (String) -> Unit,
) : BaseInputConnection(view, true) {
    private val composing = Editable.Factory.getInstance().newEditable("")
    private fun active() = view.isAttachedToWindow && isCurrent()
    private fun showComposing() { onComposing(composing.toString()); view.invalidate() }
    override fun getEditable(): Editable = composing
    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (!active()) return false
        val result = super.setComposingText(text, newCursorPosition)
        showComposing()
        return result
    }
    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (!active()) { composing.clear(); return false }
        val accepted = if (text?.toString() == "\n") target.key(KeyEvent.KEYCODE_ENTER)
            else target.text(text?.toString().orEmpty())
        composing.clear()
        Selection.setSelection(composing, 0)
        showComposing()
        return accepted
    }
    override fun finishComposingText(): Boolean {
        if (!active()) { composing.clear(); return false }
        if (composing.isNotEmpty() && !commitText(composing.toString(), 1)) return false
        return super.finishComposingText()
    }
    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (!active()) return false
        if (composing.isNotEmpty()) {
            val result = super.deleteSurroundingText(beforeLength, afterLength)
            showComposing()
            return result
        }
        repeat(beforeLength.coerceIn(0, 128)) { if (!target.key(KeyEvent.KEYCODE_DEL)) return false }
        repeat(afterLength.coerceIn(0, 128)) { if (!target.key(KeyEvent.KEYCODE_FORWARD_DEL)) return false }
        return true
    }
    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (!active()) return false
        if (composing.isEmpty()) return deleteSurroundingText(beforeLength, afterLength)
        val result = super.deleteSurroundingTextInCodePoints(beforeLength, afterLength)
        showComposing()
        return result
    }
    override fun sendKeyEvent(event: KeyEvent): Boolean = active() && view.dispatchKeyEvent(event)
    override fun performEditorAction(editorAction: Int): Boolean {
        if (!active()) return false
        if (!finishComposingText()) return false
        return target.key(KeyEvent.KEYCODE_ENTER)
    }
    override fun performContextMenuAction(id: Int): Boolean {
        if (!active()) return false
        if (id == android.R.id.paste) {
            val clip = view.context.getSystemService(ClipboardManager::class.java).primaryClip ?: return false
            return clip.itemCount > 0 && target.paste(clip.getItemAt(0).coerceToText(view.context).toString())
        }
        return false
    }
}
