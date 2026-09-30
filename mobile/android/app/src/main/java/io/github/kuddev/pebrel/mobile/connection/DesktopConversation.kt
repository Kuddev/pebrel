package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicLong

data class ConversationIdentity(val kind: String, val session: String, val epoch: Long? = null) {
    fun json() = JSONObject().put("kind", kind).put("session_id", session).apply { epoch?.let { put("epoch", it) } }
}

data class ConversationMessage(val id: String, val role: String, val text: String, val name: String?,
                               val detail: String?, val complete: Boolean, val truncated: Boolean)
data class ConversationPrompt(val id: String, val text: String, val options: List<String>, val selected: Int, val binary: Boolean = false)
data class ConversationPage(val identity: ConversationIdentity, val messages: List<ConversationMessage> = emptyList(),
                            val revision: String? = null, val before: Long? = null, val cwd: String = "",
                            val prompt: ConversationPrompt? = null, val canSend: Boolean = false,
                            val state: String = "unknown", val loaded: Boolean = false, val truncated: Boolean = false)

/** Only recent, in-memory views survive navigation. Native transcripts stay on the computer. */
class ConversationCache {
    private val pages = LinkedHashMap<String, ConversationPage>()
    @Synchronized fun get(key: String): ConversationPage? = pages[key]?.let {
        it.copy(identity = it.identity.copy(epoch = null), revision = null, canSend = false, prompt = null)
    }
    @Synchronized fun put(key: String, page: ConversationPage) {
        pages.remove(key)
        pages[key] = page
        while (pages.size > 4) pages.remove(pages.keys.first())
    }
}

class DesktopConversation internal constructor(
    initial: ConversationIdentity,
    private val key: String,
    private val cache: ConversationCache,
    private val request: suspend (String, JSONObject) -> JSONObject,
    private val availability: (Boolean) -> String?,
) {
    private val current = MutableStateFlow(cache.get(key) ?: ConversationPage(initial))
    val state = current.asStateFlow()
    private val reads = Mutex()
    private val mutations = Mutex()
    private val mutationVersion = AtomicLong()
    private fun available(write: Boolean) { availability(write)?.let { throw DesktopRpcFailure(it) } }

    suspend fun refresh(older: Boolean = false) = reads.withLock {
        available(false)
        val version = mutationVersion.get()
        if (version % 2L != 0L) return@withLock
        val old = current.value
        if (older && old.before == null) return@withLock
        val params = JSONObject().put("identity", old.identity.json())
        if (older) params.put("before", old.before) else old.revision?.let { params.put("revision", it) }
        val response = request("conversation.read", params)
        currentCoroutineContext().ensureActive()
        available(false)
        // 发送前开始的慢读取不可重新启用旧选项/发送按钮；提交中的轮询也只等下一轮。
        if (version != mutationVersion.get()) return@withLock
        val native = response.getJSONObject("identity")
        val identity = ConversationIdentity(native.getString("kind"), native.getString("session_id"), native.getLong("epoch"))
        check(identity.kind == old.identity.kind && identity.session == old.identity.session)
        if (old.identity.epoch != null && old.identity.epoch != identity.epoch) throw DesktopRpcFailure("conversation_identity_changed")
        val unchanged = response.getBoolean("unchanged")
        val received = if (unchanged) emptyList() else response.getJSONArray("messages").let { rows ->
            (0 until rows.length()).map { i -> rows.getJSONObject(i).let { row ->
                ConversationMessage(row.getString("id"), row.getString("role"), row.getString("text"),
                    if (row.isNull("name")) null else row.getString("name"),
                    if (row.isNull("detail")) null else row.getString("detail"), row.getBoolean("complete"), row.getBoolean("truncated"))
            } }
        }
        val messages = when {
            unchanged -> old.messages
            older -> (received + old.messages).distinctBy { it.id }
            received.isEmpty() -> old.messages
            else -> old.messages.takeWhile { offset(it.id) < offset(received.first().id) } + received
        }
        // 往前翻页也有明确上限；超限保留已读内容，不无限堆积 WebView 与 JVM 内存。
        if (messages.size > 640 || messages.sumOf { it.text.toByteArray().size + (it.detail?.toByteArray()?.size ?: 0) } > 1024 * 1024) {
            throw DesktopRpcFailure("conversation_history_limit")
        }
        val prompt = response.optJSONObject("prompt")?.let { p ->
            val options = p.getJSONArray("options")
            ConversationPrompt(p.getString("id"), p.getString("text"), (0 until options.length()).map(options::getString), p.getInt("selected"), p.optBoolean("binary"))
        }
        val next = old.copy(identity = identity, messages = messages,
            revision = if (older) old.revision else response.getString("revision"),
            before = if (unchanged || (!older && old.loaded && old.messages.firstOrNull()?.id != received.firstOrNull()?.id)) old.before
                else if (response.isNull("before")) null else response.getLong("before"),
            cwd = response.optString("cwd", old.cwd), loaded = true,
            prompt = prompt, canSend = response.getBoolean("can_send"), state = response.getString("state"),
            truncated = old.truncated || response.optBoolean("truncated"))
        current.value = next
        cache.put(key, next)
    }

    suspend fun send(text: String) = mutations.withLock {
        available(true)
        val page = current.value
        if (!page.canSend || page.identity.epoch == null) throw DesktopRpcFailure("agent_busy")
        mutate("conversation.send", JSONObject().put("identity", page.identity.json()).put("text", text))
    }

    suspend fun choose(prompt: ConversationPrompt, index: Int) = mutations.withLock {
        available(true)
        val page = current.value
        if (page.prompt?.id != prompt.id || page.identity.epoch == null) throw DesktopRpcFailure("prompt_changed")
        mutate("conversation.choose", JSONObject().put("identity", page.identity.json())
            .put("prompt_id", prompt.id).put("option", index))
    }

    suspend fun key(key: String) = mutations.withLock {
        available(true)
        val page = current.value
        if (page.identity.epoch == null) throw DesktopRpcFailure("conversation_unavailable")
        mutate("conversation.key", JSONObject().put("identity", page.identity.json()).put("key", key))
    }

    private suspend fun mutate(method: String, params: JSONObject) {
        mutationVersion.incrementAndGet()
        current.value = current.value.copy(canSend = false, prompt = null)
        try {
            val result = request(method, params)
            available(false)
            check(result.getBoolean("accepted"))
        } finally {
            // 即使结果不确定，也先重新读取；输入不会被重试或自动重放。
            mutationVersion.incrementAndGet()
        }
    }

    private fun offset(id: String): Long = id.substringBefore(':').toLong()
}
