package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import java.io.File

/** Small application-facing adapter for the local PTY's cwd and HOME. */
object LocalTerminalStorage {
    fun access(context: Context): SharedStorageAccess.State = SharedStorageAccess.state(context)

    fun defaultHome(): File = SharedStorageAccess.defaultDirectory()

    /** Throws an explicit permission/storage error instead of selecting private app data. */
    fun home(context: Context): File = SharedStorageAccess.requireDirectory(context)

    fun homePath(context: Context): String = home(context).absolutePath
}
