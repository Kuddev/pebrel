package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONArray
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Cold-path credential storage. The app disables backup; keys stay in Android Keystore. */
class RelayProfileStore(context: Context) {
    private val preferences = context.getSharedPreferences("pebrel_relay", Context.MODE_PRIVATE)
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey("pebrel-relay", null)
        if (existing is SecretKey) return existing
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder("pebrel-relay", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    @Synchronized fun load(): List<RelayProfile> {
        val encoded = preferences.getString("profiles", null) ?: return emptyList()
        val bytes = Base64.decode(encoded, Base64.NO_WRAP)
        require(bytes.size in 29..262144)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        val array = JSONArray(String(cipher.doFinal(bytes, 12, bytes.size - 12), Charsets.UTF_8))
        require(array.length() <= 64)
        return (0 until array.length()).map { RelayProfile.parse(array.getJSONObject(it).toString()) }
    }
    @Synchronized fun save(profiles: List<RelayProfile>) {
        require(profiles.size <= 64)
        val array = JSONArray().apply { profiles.forEach { put(it.toJson()) } }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val bytes = cipher.iv + cipher.doFinal(array.toString().toByteArray())
        check(preferences.edit().putString("profiles", Base64.encodeToString(bytes, Base64.NO_WRAP)).commit())
    }
}
