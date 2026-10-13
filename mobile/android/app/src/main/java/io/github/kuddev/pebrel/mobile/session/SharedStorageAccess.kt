package io.github.kuddev.pebrel.mobile.session

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import java.io.File

/**
 * Owns the Android permission boundary for the user's shared Pebrel directory.
 * A denied or unavailable directory is an explicit state; it never becomes app-private storage.
 */
object SharedStorageAccess {
    const val DIRECTORY_NAME = "Pebrel"

    sealed class State {
        data class Ready(val directory: File) : State()
        data class RuntimePermissionRequired(val directory: File, val permissions: List<String>) : State()
        data class AllFilesAccessRequired(val directory: File) : State()
        data class Unavailable(val directory: File, val reason: Reason) : State()
    }

    enum class Reason {
        MEDIA_UNAVAILABLE,
        DIRECTORY_NOT_WRITABLE,
        DIRECTORY_CREATION_FAILED,
    }

    /** The default HOME requested for every local terminal. */
    @Suppress("DEPRECATION")
    fun defaultDirectory(): File = Environment.getExternalStorageDirectory().resolve(DIRECTORY_NAME)

    fun state(context: Context): State {
        val directory = defaultDirectory()
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                if (!Environment.isExternalStorageManager()) return State.AllFilesAccessRequired(directory)
            } else {
                val missing = runtimePermissions(context).filter {
                    context.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
                }
                if (missing.isNotEmpty()) return State.RuntimePermissionRequired(directory, missing)
            }

            if (Environment.getExternalStorageState() != Environment.MEDIA_MOUNTED) {
                return State.Unavailable(directory, Reason.MEDIA_UNAVAILABLE)
            }
            if (directory.exists()) {
                return if (directory.isDirectory && directory.canWrite()) State.Ready(directory)
                else State.Unavailable(directory, Reason.DIRECTORY_NOT_WRITABLE)
            }
            return if ((directory.mkdirs() || directory.isDirectory) && directory.canWrite()) State.Ready(directory)
            else State.Unavailable(directory, Reason.DIRECTORY_CREATION_FAILED)
        } catch (_: SecurityException) {
            State.Unavailable(directory, Reason.DIRECTORY_NOT_WRITABLE)
        }
    }

    fun requireDirectory(context: Context): File {
        val state = state(context)
        return (state as? State.Ready)?.directory ?: throw SharedStorageUnavailableException(state)
    }

    fun runtimePermissions(context: Context): List<String> = if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.Q) {
        listOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE)
    } else {
        emptyList()
    }

    /** Opens the one-time special-access screen on Android 11 and later. */
    fun allFilesAccessIntent(context: Context): Intent? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        val appIntent = Intent(
            Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
            Uri.parse("package:${context.packageName}"),
        )
        if (appIntent.resolveActivity(context.packageManager) != null) return appIntent
        val globalIntent = Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)
        return globalIntent.takeIf { it.resolveActivity(context.packageManager) != null }
    }

    /** Useful after a user has denied a runtime permission or wants to inspect app access. */
    fun appDetailsIntent(context: Context): Intent = Intent(
        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
        Uri.parse("package:${context.packageName}"),
    )
}

class SharedStorageUnavailableException(val state: SharedStorageAccess.State) :
    IllegalStateException("shared_storage_unavailable:${state.javaClass.simpleName}")
