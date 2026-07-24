package com.xiwei.sujian.platform

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import uniffi.writer_core.SecureStorageProvider
import java.io.File
import java.io.IOException
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.UnrecoverableKeyException
import java.security.cert.CertificateException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val TAG = "AndroidKeystoreSecureStorage"
private const val KEYSTORE_ALIAS = "com.xiwei.sujian.secure_storage_v1"
private const val KEYSTORE_TYPE = "AndroidKeyStore"
private const val SECRETS_DIR = "keystore_secrets"
private const val TRANSFORMATION = "AES/GCM/NoPadding"
private const val GCM_IV_LENGTH = 12
private const val GCM_TAG_LENGTH = 128

class AndroidKeystoreSecureStorage(
    private val context: Context,
) : SecureStorageProvider {

    private val secretsDir = File(context.noBackupFilesDir, SECRETS_DIR)

    init {
        if (!secretsDir.exists()) {
            secretsDir.mkdirs()
        }
        ensureKeyExists()
        migrateOldEncKey()
    }

    private fun ensureKeyExists() {
        val keyStore = KeyStore.getInstance(KEYSTORE_TYPE)
        keyStore.load(null)
        if (!keyStore.containsAlias(KEYSTORE_ALIAS)) {
            createKey()
        }
    }

    private fun createKey() {
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_TYPE
        )
        val spec = KeyGenParameterSpec.Builder(
            KEYSTORE_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .build()
        keyGenerator.init(spec)
        keyGenerator.generateKey()
    }

    private fun getKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_TYPE)
        keyStore.load(null)
        val entry = keyStore.getEntry(KEYSTORE_ALIAS, null)
            ?: throw KeyStoreException("Keystore entry not found for alias $KEYSTORE_ALIAS")
        return (entry as KeyStore.SecretKeyEntry).secretKey
    }

    override fun getSecret(key: String): List<UByte>? {
        val file = File(secretsDir, "$key.bin")
        if (!file.exists()) return null
        val data = file.readBytes()
        if (data.size < GCM_IV_LENGTH) {
            DiagnosticsLogger.w(TAG, "Truncated ciphertext for key=$key")
            return null
        }
        val iv = data.copyOfRange(0, GCM_IV_LENGTH)
        val ciphertext = data.copyOfRange(GCM_IV_LENGTH, data.size)
        return try {
            val secretKey = getKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            val spec = GCMParameterSpec(GCM_TAG_LENGTH, iv)
            cipher.init(Cipher.DECRYPT_MODE, secretKey, spec)
            val plaintext = cipher.doFinal(ciphertext)
            plaintext.toList().map { it.toUByte() }
        } catch (e: UnrecoverableKeyException) {
            DiagnosticsLogger.e(TAG, "Keystore key invalidated for key=$key", e)
            throw RuntimeException("keystore_key_invalidated")
        } catch (e: CertificateException) {
            DiagnosticsLogger.e(TAG, "Keystore certificate error for key=$key", e)
            throw RuntimeException("keystore_error")
        }
    }

    override fun setSecret(key: String, value: List<UByte>) {
        try {
            val secretKey = getKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, secretKey)
            val iv = cipher.iv
            val plaintext = ByteArray(value.size) { value[it].toByte() }
            val ciphertext = cipher.doFinal(plaintext)
            val output = ByteArray(iv.size + ciphertext.size)
            System.arraycopy(iv, 0, output, 0, iv.size)
            System.arraycopy(ciphertext, 0, output, iv.size, ciphertext.size)
            if (!secretsDir.exists()) secretsDir.mkdirs()
            File(secretsDir, "$key.bin").writeBytes(output)
        } catch (e: UnrecoverableKeyException) {
            DiagnosticsLogger.e(TAG, "Keystore key invalidated for key=$key", e)
            throw RuntimeException("keystore_key_invalidated")
        }
    }

    override fun deleteSecret(key: String) {
        val file = File(secretsDir, "$key.bin")
        if (file.exists()) {
            file.delete()
        }
    }

    private fun migrateOldEncKey() {
        val oldKeyFile = File(context.noBackupFilesDir, ".enc_key")
        if (!oldKeyFile.exists()) return
        val oldSecretsDir = File(context.noBackupFilesDir, "secrets")
        if (oldSecretsDir.exists() && oldSecretsDir.isDirectory) {
            oldSecretsDir.deleteRecursively()
            DiagnosticsLogger.i(TAG, "Migrated old .enc_key secrets: deleted old secrets dir")
        }
        oldKeyFile.delete()
        DiagnosticsLogger.i(TAG, "Migrated old .enc_key: deleted key file")
    }
}
