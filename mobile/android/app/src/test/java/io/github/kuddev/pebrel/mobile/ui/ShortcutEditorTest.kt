package io.github.kuddev.pebrel.mobile.ui

import android.app.Application
import android.content.Context
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.ShortcutPreferences
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.*

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28], application = Application::class)
@ConscryptMode(ConscryptMode.Mode.OFF)
class ShortcutEditorTest {
    @get:Rule val compose = createComposeRule()
    private val context get() = ApplicationProvider.getApplicationContext<Context>()

    @Test fun editingValidatesAndPersistsTheOrderedDefinitions() {
        context.getSharedPreferences("terminal_shortcuts", Context.MODE_PRIVATE).edit().clear().commit()
        val preferences = ShortcutPreferences(context)
        var dismissed = false
        compose.setContent { MaterialTheme { ShortcutEditor(preferences) { dismissed = true } } }
        fun chip(label: String) = compose.onNodeWithText(label).performScrollTo().performClick()
        fun add(label: String) = compose.onNodeWithText(context.getString(R.string.custom_shortcuts_add, label))
        compose.onNodeWithText(context.getString(R.string.custom_shortcuts_pick)).performScrollTo().assertIsNotEnabled()
        chip("Ctrl"); chip("Shift")
        compose.onNode(hasSetTextAction()).performScrollTo().performTextReplacement("x")
        add("Ctrl+Shift+X").performScrollTo().assertIsEnabled().performClick()
        assertTrue(preferences.state.value.isEmpty())
        chip("Alt"); chip("Enter")
        add("Alt+Enter").performScrollTo().performClick()
        chip("F1–F12"); chip("F5")
        add("F5").performScrollTo().performClick()
        // The same combination cannot be added twice.
        compose.onAllNodesWithText("F5").onLast().performScrollTo().performClick()
        add("F5").performScrollTo().assertIsNotEnabled()
        compose.onNodeWithText(context.getString(R.string.custom_shortcuts_duplicate)).assertExists()
        compose.onNodeWithText(context.getString(R.string.save)).assertIsEnabled().performClick()
        assertTrue(dismissed)
        assertEquals(listOf("Ctrl+Shift+X", "Alt+Enter", "F5"), ShortcutPreferences(context).state.value)
    }

    @Test fun duplicateAndOverlongDefinitionsDoNotReplaceTheSavedList() {
        val preferences = ShortcutPreferences(context)
        assertTrue(preferences.save(listOf("Shift+Ctrl+X")))
        assertFalse(preferences.save(listOf("Ctrl+Esc", "Ctrl+Escape")))
        assertFalse(preferences.save(List(25) { "F1" }))
        assertFalse(preferences.save(listOf("Ctrl+oops")))
        assertEquals(listOf("Ctrl+Shift+X"), ShortcutPreferences(context).state.value)
        assertTrue(preferences.save(emptyList()))
        assertTrue(ShortcutPreferences(context).state.value.isEmpty())
    }
}
