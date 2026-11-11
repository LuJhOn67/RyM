package com.rymplayer.data

import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The Google sign-in, encrypted with a key the Android Keystore keeps (AES-256-GCM; the key
 * never leaves the phone's secure storage). An old plain "cookie" is encrypted the first time
 * it's read, then removed. If the key is ever gone (a backup restored on another phone), the
 * sign-in can't be opened and you sign in again.
 */
object SecureCookie {
    private const val KEY_NAME = "rym_sign_in"
    private const val SEALED = "cookie_sealed"
    private const val PLAIN = "cookie"

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(KEY_NAME, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(KEY_NAME, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    private fun seal(text: String): String {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        val out = c.iv + c.doFinal(text.toByteArray(Charsets.UTF_8))
        return Base64.encodeToString(out, Base64.NO_WRAP)
    }

    private fun open(sealed: String): String? = runCatching {
        val bytes = Base64.decode(sealed, Base64.NO_WRAP)
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, 12))
        String(c.doFinal(bytes, 12, bytes.size - 12), Charsets.UTF_8)
    }.getOrNull()

    /** The saved cookie header, if signed in. */
    fun read(prefs: SharedPreferences): String? {
        prefs.getString(SEALED, null)?.let { sealed ->
            open(sealed)?.let { return it }
            // The key is gone: this sign-in can't be opened any more.
            prefs.edit().remove(SEALED).apply()
            return null
        }
        val plain = prefs.getString(PLAIN, null)?.takeIf { it.isNotBlank() } ?: return null
        write(prefs, plain)
        return plain
    }

    /** Saves the cookie header encrypted (null: signed out, everything removed). */
    fun write(prefs: SharedPreferences, cookie: String?) {
        if (cookie == null) {
            prefs.edit().remove(SEALED).remove(PLAIN).apply()
            return
        }
        val sealed = runCatching { seal(cookie) }.getOrNull()
        if (sealed != null) prefs.edit().putString(SEALED, sealed).remove(PLAIN).apply()
        // No Keystore (very rare): kept as before rather than signing you out.
        else prefs.edit().putString(PLAIN, cookie).apply()
    }
}
