package io.github.kuddev.pebrel.terminal

import android.text.Editable
import android.text.Selection
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection

/** IME preedit stays local; only committed text enters the remote byte stream. */
internal class GhosttyInputConnection(private val view: GhosttyView) : BaseInputConnection(view, true) {
    private val composing = Editable.Factory.getInstance().newEditable("")
    private val owner = view.session
    private fun active() = view.isAttachedToWindow && view.directInput && view.session === owner
    override fun getEditable(): Editable = composing
    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (!active()) return false
        val result = super.setComposingText(text, newCursorPosition)
        view.composingText = composing.toString()
        view.invalidate()
        return result
    }
    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (!active()) { composing.clear(); return false }
        view.accept(if (text?.toString() == "\n") view.session?.key(KeyEvent.KEYCODE_ENTER) == true
            else view.session?.sendText(text?.toString().orEmpty()) == true)
        composing.clear()
        Selection.setSelection(composing, 0)
        view.composingText = ""
        view.invalidate()
        return true
    }
    override fun finishComposingText(): Boolean {
        if (composing.isNotEmpty()) commitText(composing.toString(), 1)
        return super.finishComposingText()
    }
    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (!active()) return false
        if (composing.isNotEmpty()) {
            val result = super.deleteSurroundingText(beforeLength, afterLength)
            view.composingText = composing.toString()
            view.invalidate()
            return result
        }
        repeat(beforeLength.coerceIn(0, 128)) { view.accept(view.session?.key(KeyEvent.KEYCODE_DEL) == true) }
        repeat(afterLength.coerceIn(0, 128)) { view.accept(view.session?.key(KeyEvent.KEYCODE_FORWARD_DEL) == true) }
        return true
    }
    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (!active()) return false
        if (composing.isEmpty()) return deleteSurroundingText(beforeLength, afterLength)
        val result = super.deleteSurroundingTextInCodePoints(beforeLength, afterLength)
        view.composingText = composing.toString()
        view.invalidate()
        return result
    }
    override fun sendKeyEvent(event: KeyEvent): Boolean = active() && view.dispatchKeyEvent(event)
    override fun performEditorAction(editorAction: Int): Boolean {
        if (!active()) return false
        finishComposingText()
        view.accept(view.session?.key(KeyEvent.KEYCODE_ENTER) == true)
        return true
    }
    override fun performContextMenuAction(id: Int): Boolean {
        if (!active()) return false
        if (id == android.R.id.paste) { view.pasteClipboard(); return true }
        return false
    }
}
