package io.github.kuddev.pebrel.mobile.connection

import android.util.Base64
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.ByteArrayOutputStream

data class DesktopFileContent(val bytes: ByteArray, val revision: String, val kind: String)

/** A view-owned handle: reconnecting never silently retargets an in-flight file or close request. */
class DesktopTabs internal constructor(
    private val request: suspend (String, JSONObject) -> JSONObject,
    private val availability: (Boolean, DesktopTab?) -> String?,
) {
    private fun checkAvailable(write: Boolean, tab: DesktopTab? = null) {
        availability(write, tab)?.let { throw DesktopRpcFailure(it) }
        if (tab != null && tab.id == null) throw DesktopRpcFailure("tabs_unsupported")
    }

    private suspend fun mutate(method: String, params: JSONObject, tab: DesktopTab? = null): JSONObject {
        checkAvailable(true, tab)
        val result = request(method, params)
        // 关闭会主动移除目标，因此返回时只复核连接，不能再要求被关闭的标签仍存在。
        checkAvailable(false)
        return result
    }

    suspend fun focus(tab: DesktopTab) = mutate("tab.focus", target(tab), tab)
    suspend fun close(tab: DesktopTab) = mutate("tab.close", target(tab), tab)

    suspend fun newTerminal(window: Long?, cwd: String?): DesktopTab {
        val params = JSONObject()
        window?.let { params.put("window_id", it) }
        cwd?.takeIf(String::isNotBlank)?.let { params.put("cwd", it) }
        val result = mutate("tab.new", params)
        val pane = result.getJSONObject("action").getLong("pane_id")
        val resultWindow = result.getJSONObject("action").getLong("window_id")
        return parseDesktopTabs(result.getJSONObject("snapshot")).first { it.window == resultWindow && it.panes.any { p -> p.id == pane } }
    }

    suspend fun openFile(window: Long, path: String): DesktopTab {
        val result = mutate("tab.open", JSONObject().put("window_id", window).put("path", path))
        val id = result.getJSONObject("action").getString("tab_id")
        return parseDesktopTabs(result.getJSONObject("snapshot")).first { it.window == window && it.id == id }
    }

    suspend fun read(tab: DesktopTab, onProgress: (Int, Int) -> Unit = { _, _ -> }): DesktopFileContent = withContext(Dispatchers.IO) {
        val output = ByteArrayOutputStream()
        var revision: String? = null
        var total: Int? = null
        var kind: String? = null
        while (true) {
            currentCoroutineContext().ensureActive()
            checkAvailable(false, tab)
            val params = target(tab).put("offset", output.size()).put("limit", 65536)
            revision?.let { params.put("revision", it) }
            val result = request("tab.read", params)
            checkAvailable(false, tab)
            val nextRevision = result.getString("revision")
            val nextTotal = result.getInt("total_bytes")
            val nextKind = result.getString("kind")
            val bytes = Base64.decode(result.getString("data"), Base64.NO_WRAP)
            check(result.getString("tab_id") == tab.id && result.getLong("window_id") == tab.window)
            check(nextTotal in 0..MAX_FILE_BYTES && bytes.size <= 65536)
            check(revision == null || revision == nextRevision)
            check(total == null || total == nextTotal)
            check(kind == null || kind == nextKind)
            check(result.getInt("offset") == output.size() && result.getInt("next_offset") == output.size() + bytes.size)
            check(output.size() + bytes.size <= nextTotal)
            revision = nextRevision; total = nextTotal; kind = nextKind
            output.write(bytes)
            onProgress(output.size(), nextTotal)
            if (result.getBoolean("eof")) {
                check(output.size() == nextTotal)
                return@withContext DesktopFileContent(output.toByteArray(), nextRevision, nextKind)
            }
            check(bytes.isNotEmpty())
        }
        @Suppress("UNREACHABLE_CODE") error("unreachable")
    }

    private fun target(tab: DesktopTab) = JSONObject().put("window_id", tab.window).put("tab_id", tab.id)

    companion object { const val MAX_FILE_BYTES = 16 * 1024 * 1024 }
}
