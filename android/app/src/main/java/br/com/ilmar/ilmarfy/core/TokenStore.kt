package br.com.ilmar.ilmarfy.core

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Guarda o token do aparelho cifrado com uma chave do Android Keystore (não sai do hardware do celular).
 * A senha nunca é guardada: só o token, que pode ser revogado no servidor.
 */
class TokenStore(context: Context) {
    private val prefs = context.getSharedPreferences("ilmarfy_auth", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return gen.generateKey()
    }

    var token: String?
        get() {
            val data = prefs.getString("token", null) ?: return null
            val iv = prefs.getString("iv", null) ?: return null
            return runCatching {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)))
                String(cipher.doFinal(Base64.decode(data, Base64.NO_WRAP)), Charsets.UTF_8)
            }.getOrNull()
        }
        set(value) {
            if (value == null) {
                prefs.edit().remove("token").remove("iv").apply()
                return
            }
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            prefs.edit()
                .putString("token", Base64.encodeToString(cipher.doFinal(value.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP))
                .putString("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
                .apply()
        }

    var server: String
        get() = prefs.getString("server", null) ?: DEFAULT_SERVER
        set(value) = prefs.edit().putString("server", value.trim().trimEnd('/')).apply()

    var deviceName: String?
        get() = prefs.getString("device_name", null)
        set(value) = prefs.edit().putString("device_name", value).apply()

    companion object {
        const val DEFAULT_SERVER = "https://download.ilmar.com.br"
        private const val KEY_ALIAS = "ilmarfy_token_key"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
