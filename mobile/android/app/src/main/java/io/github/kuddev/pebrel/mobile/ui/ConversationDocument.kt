package io.github.kuddev.pebrel.mobile.ui

import io.github.kuddev.pebrel.mobile.connection.ConversationMessage

internal fun prepareConversation(messages: List<ConversationMessage>, colors: ReaderColors, agent: String,
                                 userLabel: String, toolLabel: String, pendingLabel: String,
                                 clippedLabel: String, copyLabel: String, imageLabel: String): ReaderDocument {
    val markup = ReaderMarkup(copyLabel, imageLabel)
    val body = buildString {
        append("<section class=\"conversation\">")
        messages.forEach { message ->
            val id = readerEscape(message.id)
            if (message.role == "tool") {
                append("<details class=\"tool-message\" data-message id=\"m-$id\"><summary>")
                append(readerEscape(message.name?.ifBlank { toolLabel } ?: toolLabel))
                if (!message.complete) append(" · ${readerEscape(pendingLabel)}")
                append("</summary>")
                if (message.text.isNotEmpty()) append(markup.codeBlock(message.text))
                message.detail?.let { append(markup.codeBlock(it)) }
                if (message.truncated) append("<p class=\"message-notice\">${readerEscape(clippedLabel)}</p>")
                append("</details>")
            } else {
                append("<article data-message id=\"m-$id\"><header class=\"message-author\">")
                append(readerEscape(if (message.role == "user") userLabel else agent))
                append("</header>")
                if (message.role == "user") append("<div class=\"user-message\">")
                append(markup.markdown(message.text))
                if (message.role == "user") append("</div>")
                if (message.truncated) append("<p class=\"message-notice\">${readerEscape(clippedLabel)}</p>")
                append("</article>")
            }
        }
        append("</section>")
    }
    return markup.document(body, colors)
}
