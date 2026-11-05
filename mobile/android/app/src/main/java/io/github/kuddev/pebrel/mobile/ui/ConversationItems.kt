package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.Immutable
import io.github.kuddev.pebrel.mobile.connection.ConversationMessage
import org.json.JSONArray
import org.json.JSONObject

internal enum class ToolKind { Shell, Read, Edit, Search, Web, Task, Todo, Question, Other }

/** Waiting means a call has no result while the Agent asks for an interaction, usually approval. */
internal enum class ToolState { Running, Waiting, Pending, Done, Failed }

@Immutable
internal data class AskedOption(val label: String, val description: String)

@Immutable
internal data class AskedQuestion(val header: String, val question: String, val options: List<AskedOption>,
                                  val multiple: Boolean)

/** One timeline row. Keys are the transcript's own message ids, so rows keep their state across reads. */
@Immutable
internal sealed interface ChatItem {
    val key: String

    data class User(override val key: String, val source: String, val document: ChatDocument,
                    val truncated: Boolean) : ChatItem

    data class Assistant(override val key: String, val source: String, val document: ChatDocument,
                         val truncated: Boolean, val startsTurn: Boolean) : ChatItem

    data class Tool(override val key: String, val kind: ToolKind, val name: String?, val summary: String,
                    val input: String, val output: String?, val state: ToolState, val truncated: Boolean) : ChatItem

    /** A structured question shown natively. Answers stay with the desktop's prompt projection. */
    data class Question(override val key: String, val questions: List<AskedQuestion>, val answered: Boolean) : ChatItem
}

/** Rebuilds the timeline, parsing only messages whose text changed since [previous]. */
internal class ConversationTimelineBuilder(private val style: ChatMarkdownStyle) {
    private var documents: Map<String, Pair<String, ChatDocument>> = emptyMap()

    @Synchronized
    fun build(messages: List<ConversationMessage>, state: String): List<ChatItem> {
        val parsed = HashMap<String, Pair<String, ChatDocument>>(messages.size)
        fun document(message: ConversationMessage): ChatDocument {
            val cached = documents[message.id]?.takeIf { it.first == message.text }?.second
            val result = cached ?: parseChatMarkdown(message.text, style)
            parsed[message.id] = message.text to result
            return result
        }
        var previousRole: String? = null
        val items = messages.map { message ->
            val item = when (message.role) {
                "user" -> ChatItem.User(message.id, message.text, document(message), message.truncated)
                "assistant" -> ChatItem.Assistant(message.id, message.text, document(message), message.truncated,
                    startsTurn = previousRole != "assistant" && previousRole != "tool")
                else -> toolItem(message, state)
            }
            previousRole = message.role
            item
        }
        documents = parsed
        return items.distinctBy { it.key }
    }
}

internal fun toolItem(message: ConversationMessage, conversationState: String): ChatItem {
    val kind = toolKind(message.name)
    if (kind == ToolKind.Question) {
        askedQuestions(message.text)?.let { return ChatItem.Question(message.id, it, message.complete) }
    }
    val state = when {
        message.error -> ToolState.Failed
        message.complete -> ToolState.Done
        conversationState == "running" -> ToolState.Running
        conversationState == "waiting_input" || conversationState == "attention" -> ToolState.Waiting
        else -> ToolState.Pending
    }
    return ChatItem.Tool(message.id, kind, message.name, toolSummary(kind, message.text), message.text,
        message.detail, state, message.truncated)
}

private fun normalized(name: String?): String = name.orEmpty().substringAfterLast("__").lowercase().filter(Char::isLetterOrDigit)

/** Claude Code and Codex name the same capabilities differently; MCP tools keep their own names. */
internal fun toolKind(name: String?): ToolKind = when (normalized(name)) {
    "bash", "shell", "localshell", "execcommand", "exec", "runcommand", "terminal", "writestdin", "bashoutput", "killshell" -> ToolKind.Shell
    "read", "readfile", "view", "notebookread", "readmanyfiles" -> ToolKind.Read
    "edit", "multiedit", "write", "writefile", "notebookedit", "applypatch", "strreplacebasededittool", "createfile" -> ToolKind.Edit
    "grep", "glob", "search", "find", "ls", "listdir", "listfiles", "codebasesearch", "filesearch" -> ToolKind.Search
    "webfetch", "websearch", "fetch", "browse", "websearchpreview" -> ToolKind.Web
    "task", "agent", "subagent", "spawnagent" -> ToolKind.Task
    "todowrite", "todoread", "updateplan" -> ToolKind.Todo
    "askuserquestion" -> ToolKind.Question
    else -> ToolKind.Other
}

/** `mcp__server__tool` reads as `server · tool`; other names are shown as recorded. */
internal fun toolDisplayName(name: String?): String? {
    if (name == null || !name.startsWith("mcp__")) return name
    val parts = name.removePrefix("mcp__").split("__", limit = 2)
    return if (parts.size == 2) "${parts[0]} · ${parts[1]}" else name
}

private const val SUMMARY_LENGTH = 160

/** One line saying what the call acts on, taken from its own arguments. Never guessed from output. */
internal fun toolSummary(kind: ToolKind, input: String): String {
    val text = input.trim()
    val json = if (text.startsWith("{")) runCatching { JSONObject(text) }.getOrNull() else null
    fun field(vararg keys: String): String? = keys.firstNotNullOfOrNull { key ->
        when (val value = json?.opt(key)) {
            is String -> value.takeIf(String::isNotBlank)
            is JSONArray -> (0 until value.length()).joinToString(" ") { value.optString(it) }.takeIf(String::isNotBlank)
            else -> null
        }
    }
    val summary = when (kind) {
        ToolKind.Shell -> field("command", "cmd", "script", "chars")
        ToolKind.Read -> field("file_path", "path", "notebook_path", "paths")
        ToolKind.Edit -> field("file_path", "path", "notebook_path") ?: patchFiles(json?.optString("input") ?: text)
        ToolKind.Search -> field("pattern", "query", "glob", "regex")?.let { pattern ->
            field("path")?.let { "$pattern · $it" } ?: pattern
        }
        ToolKind.Web -> field("url", "query")
        ToolKind.Task -> field("description", "prompt")
        ToolKind.Todo -> todoSummary(json)
        ToolKind.Question, ToolKind.Other -> null
    } ?: json?.let(::firstText) ?: text.takeUnless { it.startsWith("{") || it.startsWith("[") }
    return summary.orEmpty().lineSequence().map(String::trim).firstOrNull(String::isNotEmpty).orEmpty()
        .replace(Regex("\\s+"), " ").let { if (it.length > SUMMARY_LENGTH) it.take(SUMMARY_LENGTH - 1) + "…" else it }
}

private fun patchFiles(patch: String): String? = Regex("^\\*\\*\\* (?:Add|Update|Delete) File: (.+)$", RegexOption.MULTILINE)
    .findAll(patch).map { it.groupValues[1].trim() }.toList().takeIf { it.isNotEmpty() }?.joinToString(", ")

private fun todoSummary(json: JSONObject?): String? {
    val todos = json?.optJSONArray("todos") ?: json?.optJSONArray("plan") ?: return null
    val rows = (0 until todos.length()).mapNotNull { todos.optJSONObject(it) }
    val current = rows.firstOrNull { it.optString("status") == "in_progress" } ?: rows.firstOrNull()
    return current?.let { it.optString("content").ifBlank { it.optString("step") } }?.takeIf(String::isNotBlank)
}

private fun firstText(json: JSONObject): String? = json.keys().asSequence()
    .mapNotNull { key -> (json.opt(key) as? String)?.takeIf(String::isNotBlank) }.firstOrNull()

/** The AskUserQuestion arguments: `questions[{question, header, options[{label, description}], multiSelect}]`. */
internal fun askedQuestions(input: String): List<AskedQuestion>? {
    val root = runCatching { JSONObject(input.trim()) }.getOrNull() ?: return null
    val questions = root.optJSONArray("questions") ?: return null
    val parsed = (0 until questions.length()).mapNotNull { index ->
        val item = questions.optJSONObject(index) ?: return@mapNotNull null
        val text = item.optString("question").trim()
        if (text.isEmpty()) return@mapNotNull null
        val options = item.optJSONArray("options")
        AskedQuestion(item.optString("header").trim(), text,
            (0 until (options?.length() ?: 0)).mapNotNull { option ->
                when (val value = options?.opt(option)) {
                    is JSONObject -> value.optString("label").trim().takeIf(String::isNotEmpty)
                        ?.let { AskedOption(it, value.optString("description").trim()) }
                    is String -> value.trim().takeIf(String::isNotEmpty)?.let { AskedOption(it, "") }
                    else -> null
                }
            }, item.optBoolean("multiSelect"))
    }
    return parsed.takeIf { it.isNotEmpty() }
}

/**
 * A row signalling that the Agent is working when nothing else shows it: a running
 * call already has its own spinner, and a pending question needs the user instead.
 */
internal fun showsWorking(items: List<ChatItem>, state: String): Boolean {
    if (state != "running") return false
    val last = items.lastOrNull()
    return !(last is ChatItem.Tool && last.state == ToolState.Running)
}
