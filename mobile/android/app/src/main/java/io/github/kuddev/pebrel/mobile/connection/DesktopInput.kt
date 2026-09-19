package io.github.kuddev.pebrel.mobile.connection

import org.json.JSONObject

internal data class DesktopBridgeCapabilities(
    val allowInput: Boolean,
    val canSendKeys: Boolean,
    val canCreateTabs: Boolean,
    val canCloseTabs: Boolean,
)

internal fun desktopBridgeCapabilities(
    requestedInput: Boolean,
    advertised: JSONObject?,
): DesktopBridgeCapabilities {
    val allowInput = requestedInput && advertised?.optBoolean("input") == true
    return DesktopBridgeCapabilities(
        allowInput = allowInput,
        canSendKeys = allowInput && advertised?.optBoolean("send_keys") == true,
        canCreateTabs = allowInput && advertised?.optBoolean("tab_create") == true,
        canCloseTabs = allowInput && advertised?.optBoolean("tab_close") == true,
    )
}

internal data class DesktopControlKey(val key: String, val control: Boolean = false)

internal fun desktopControlKey(label: String): DesktopControlKey? = when (label) {
    "Esc" -> DesktopControlKey("escape")
    "Tab" -> DesktopControlKey("tab")
    "Ctrl+C" -> DesktopControlKey("c", control = true)
    "←" -> DesktopControlKey("left")
    "→" -> DesktopControlKey("right")
    "↑" -> DesktopControlKey("up")
    "↓" -> DesktopControlKey("down")
    else -> null
}

internal fun desktopInputFailure(code: String): String = when (code) {
    "delivery_unknown", "runtime_connection_lost" -> "delivery_unknown"
    else -> "input_rejected"
}
