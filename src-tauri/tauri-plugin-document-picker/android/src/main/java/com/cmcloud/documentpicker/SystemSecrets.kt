package com.cmcloud.documentpicker

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Non-exportable, app-scoped Android Keystore wrapping for AxuanStor secrets. */
internal object SystemSecrets {
    private const val alias = "cmcloudplus.axuanstor.system-secrets.v1"
    private val magic = "ASOS1".toByteArray(Charsets.US_ASCII)
    @Synchronized
    fun clear() {
        val store = KeyStore.getInstance("AndroidKeyStore")
        store.load(null)
        if (store.containsAlias(alias)) store.deleteEntry(alias)
    }

    @Synchronized
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore")
        store.load(null)
        if (store.containsAlias(alias)) return store.getKey(alias, null) as SecretKey
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build()
        )
        return generator.generateKey()
    }

    @Synchronized
    fun transform(id: String, input: ByteArray, decrypt: Boolean): ByteArray {
        require(id.matches(Regex("[A-Za-z0-9._:-]{1,128}")))
        require(input.size <= 131072)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val aad = "CMCloud++/AxuanStor/system-secrets/v1/$id".toByteArray(Charsets.UTF_8)
        if (decrypt) {
            require(input.size >= magic.size + 12 + 16)
            require(input.copyOfRange(0, magic.size).contentEquals(magic))
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, input.copyOfRange(magic.size, magic.size + 12)))
            cipher.updateAAD(aad)
            return cipher.doFinal(input, magic.size + 12, input.size - magic.size - 12)
        }
        cipher.init(Cipher.ENCRYPT_MODE, key())
        require(cipher.iv.size == 12)
        cipher.updateAAD(aad)
        return magic + cipher.iv + cipher.doFinal(input)
    }
}
