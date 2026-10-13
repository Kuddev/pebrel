package io.github.kuddev.pebrel.mobile

import android.app.Application
import io.github.kuddev.pebrel.mobile.session.SessionRepository
import io.github.kuddev.pebrel.mobile.session.TerminalFonts

data class HostIconSpec(val id: String, val glyph: String, val zh: String, val en: String)

class PebrelApplication : Application() {
    val sessions by lazy { SessionRepository(this) }
    val terminalTypeface by lazy {
        TerminalFonts.typeface(this, "maple")
    }
    fun terminalTypeface(family: String): android.graphics.Typeface = TerminalFonts.typeface(this, family)
    val themes by lazy {
        org.json.JSONObject(assets.open("themes.json").bufferedReader().use { it.readText() })
    }
    val hostIcons by lazy {
        val rows = org.json.JSONArray(assets.open("host-icons.json").bufferedReader().use { it.readText() })
        (0 until rows.length()).map { rows.getJSONObject(it).let { row ->
            HostIconSpec(row.getString("id"), row.getString("glyph"), row.getString("zh"), row.getString("en"))
        } }
    }
}
