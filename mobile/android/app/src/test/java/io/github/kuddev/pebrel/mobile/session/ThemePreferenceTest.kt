package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class ThemePreferenceTest {
    private val context get() = ApplicationProvider.getApplicationContext<Context>()
    private val preferences get() = context.getSharedPreferences("pebrel_mobile", Context.MODE_PRIVATE)

    @Before fun clearPreferences() {
        preferences.edit().clear().commit()
    }

    @Test fun freshInstallUsesMintLight() {
        assertEquals("MintLight", SessionRepository(context).theme.value)
    }

    @Test fun upgradeKeepsEveryExplicitThemeChoiceIncludingSystem() {
        for (choice in listOf("Paper", "Nord", "MintLight", "system")) {
            preferences.edit().putString("theme", choice).commit()
            assertEquals(choice, SessionRepository(context).theme.value)
            assertEquals(choice, preferences.getString("theme", null))
        }
    }
}
