package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.net.Uri
import io.github.kuddev.pebrel.ssh.NativeSshException

/** Called by SSH's IO worker, using only the document explicitly selected by the user. */
fun sshKeySource(context: Context, host: HostProfile): (() -> ByteArray)? =
    host.keyUri.takeIf(String::isNotBlank)?.let { stored ->
        val resolver = context.applicationContext.contentResolver
        val source: () -> ByteArray = {
            val uri = Uri.parse(stored)
            if (uri.scheme != "content") throw NativeSshException("KEY")
            val buffer = ByteArray(65_537)
            try {
                resolver.openInputStream(uri)?.use { input ->
                    var count = 0
                    while (count < buffer.size) {
                        val read = input.read(buffer, count, buffer.size - count)
                        if (read < 0) break
                        if (read == 0) throw NativeSshException("KEY")
                        count += read
                    }
                    if (count !in 1..65_536) throw NativeSshException("KEY")
                    buffer.copyOf(count)
                } ?: throw NativeSshException("KEY")
            } catch (error: Exception) {
                throw NativeSshException("KEY", error)
            } finally { buffer.fill(0) }
        }
        source
    }
