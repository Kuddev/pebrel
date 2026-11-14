package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.flow.StateFlow

/** A live Agent conversation, whether a paired computer or an SSH server owns the Agent. */
interface ConversationSource {
    val state: StateFlow<ConversationPage>

    /** Returns whether the page changed, so the caller can poll sooner while output is arriving. */
    suspend fun refresh(older: Boolean = false): Boolean
    suspend fun send(text: String)
    suspend fun choose(prompt: ConversationPrompt, index: Int)
    suspend fun key(key: String)
}
