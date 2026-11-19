package io.github.kuddev.pebrel.mobile.session

/** One live session's accepted commands. Counts and text stay in memory, with LRU bounds. */
internal class CommandUsage {
    private val commands = linkedMapOf<String, Long>()

    fun record(submitted: String): List<String> {
        val command = submitted.trim()
        if (command.isNotEmpty() && command.length <= 32_768) {
            val uses = commands.remove(command) ?: 0
            commands[command] = if (uses == Long.MAX_VALUE) uses else uses + 1
            var size = commands.keys.sumOf(String::length)
            val oldest = commands.iterator()
            while (commands.size > 40 || size > 32_768) {
                size -= oldest.next().key.length
                oldest.remove()
            }
        }
        // Kotlin's stable sort preserves latest use first when counts are equal.
        return commands.entries.toList().asReversed().sortedByDescending { it.value }.map { it.key }
    }
}
