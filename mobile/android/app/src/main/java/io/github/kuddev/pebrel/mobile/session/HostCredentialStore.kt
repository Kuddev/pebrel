package io.github.kuddev.pebrel.mobile.session

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Encrypted records are independently replaceable; keys stay in Android Keystore. */
class HostCredentialStore(context: Context) {
    private val preferences = context.getSharedPreferences("pebrel_host_credentials", Context.MODE_PRIVATE)

    @Synchronized
    fun ids(): Set<String> = preferences.all.keys.filter { it.startsWith(PREFIX) }.map { it.removePrefix(PREFIX) }.toSet()

    @Synchronized
    fun load(hostKey: String): CharArray? {
        val encoded = preferences.getString(record(hostKey), null) ?: return null
        require(encoded.length <= MAX_RECORD_CHARS)
        val bytes = Base64.decode(encoded, Base64.NO_WRAP)
        require(bytes.size >= IV_BYTES + 16)
        val cipher = Cipher.getInstance(CIPHER)
        cipher.init(Cipher.DECRYPT_MODE, key(create = false), GCMParameterSpec(128, bytes.copyOfRange(0, IV_BYTES)))
        cipher.updateAAD(hostKey.toByteArray(StandardCharsets.UTF_8))
        val plain = cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES)
        return try {
            val decoded = StandardCharsets.UTF_8.decode(ByteBuffer.wrap(plain))
            try { CharArray(decoded.remaining()).also { decoded.get(it) } }
            finally { if (decoded.hasArray()) decoded.array().fill('\u0000') }
        } finally { plain.fill(0) }
    }

    @Synchronized
    fun save(hostKey: String, password: CharArray) {
        require(password.isNotEmpty() && password.size <= 1024)
        require(preferences.contains(record(hostKey)) || ids().size < 256)
        val buffer = StandardCharsets.UTF_8.encode(CharBuffer.wrap(password))
        val plain = ByteArray(buffer.remaining()).also { buffer.get(it) }
        if (buffer.hasArray()) buffer.array().fill(0)
        val encrypted = try {
            val cipher = Cipher.getInstance(CIPHER)
            cipher.init(Cipher.ENCRYPT_MODE, key(create = true))
            cipher.updateAAD(hostKey.toByteArray(StandardCharsets.UTF_8))
            cipher.iv + cipher.doFinal(plain)
        } finally { plain.fill(0) }
        val encoded = Base64.encodeToString(encrypted, Base64.NO_WRAP)
        encrypted.fill(0)
        check(preferences.edit().putString(record(hostKey), encoded).commit())
    }

    @Synchronized
    fun clear(hostKey: String) {
        check(preferences.edit().remove(record(hostKey)).commit())
    }

    private fun record(hostKey: String): String {
        require(hostKey.length == 64 && hostKey.all { it in '0'..'9' || it in 'a'..'f' })
        return PREFIX + hostKey
    }

    private fun key(create: Boolean): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey(KEY_ALIAS, null)
        if (existing is SecretKey) return existing
        check(create) { "credential_key_missing" }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setUserAuthenticationRequired(false).build())
        }.generateKey()
    }

    private companion object {
        const val PREFIX = "password_"
        const val KEY_ALIAS = "pebrel-host-passwords"
        const val CIPHER = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val MAX_RECORD_CHARS = 8192
    }
}
