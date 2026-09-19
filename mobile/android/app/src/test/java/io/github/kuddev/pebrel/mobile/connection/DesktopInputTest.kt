package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class DesktopInputTest {
    @Test
    fun mapsOnlyTheControlKeysExposedByTheComposer() {
        val expected = mapOf(
            "Esc" to DesktopControlKey("escape"),
            "Tab" to DesktopControlKey("tab"),
            "Ctrl+C" to DesktopControlKey("c", control = true),
            "←" to DesktopControlKey("left"),
            "→" to DesktopControlKey("right"),
            "↑" to DesktopControlKey("up"),
            "↓" to DesktopControlKey("down"),
        )

        expected.forEach { (label, key) -> assertEquals(key, desktopControlKey(label)) }
        assertNull(desktopControlKey("Enter"))
        assertNull(desktopControlKey("arbitrary bytes"))
    }

    @Test
    fun negotiatedFeaturesRequireBothPermissionAndAnExplicitCapability() {
        val advertised = JSONObject()
            .put("input", true)
            .put("send_keys", true)
            .put("tab_create", true)
            .put("tab_close", true)

        val enabled = desktopBridgeCapabilities(requestedInput = true, advertised = advertised)
        assertTrue(enabled.allowInput)
        assertTrue(enabled.canSendKeys)
        assertTrue(enabled.canCreateTabs)
        assertTrue(enabled.canCloseTabs)

        val declined = desktopBridgeCapabilities(requestedInput = false, advertised = advertised)
        assertFalse(declined.allowInput)
        assertFalse(declined.canSendKeys)
        assertFalse(declined.canCreateTabs)
        assertFalse(declined.canCloseTabs)
    }

    @Test
    fun anOlderDesktopKeepsPromptInputWithoutClaimingNewControls() {
        val legacy = desktopBridgeCapabilities(
            requestedInput = true,
            advertised = JSONObject().put("input", true),
        )

        assertTrue(legacy.allowInput)
        assertFalse(legacy.canSendKeys)
        assertFalse(legacy.canCreateTabs)
        assertFalse(legacy.canCloseTabs)
    }

    @Test
    fun uncertainDeliveryIsNotReportedAsARejectedKey() {
        assertEquals("delivery_unknown", desktopInputFailure("delivery_unknown"))
        assertEquals("delivery_unknown", desktopInputFailure("runtime_connection_lost"))
        assertEquals("input_rejected", desktopInputFailure("invalid_params"))
        assertEquals("input_rejected", desktopInputFailure("target_not_found"))
    }
}
