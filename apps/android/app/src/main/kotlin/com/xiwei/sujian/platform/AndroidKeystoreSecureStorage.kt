package com.xiwei.sujian.platform

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import uniffi.writer_core.SecureStorageException
import uniffi.writer_core.SecureStorageProvider
import java.io.File
import java.io.FileOutputStream
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.UnrecoverableKeyException
import java.security.cert.CertificateException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

private const val TAG = "AndroidKeystoreSecureStorage"
private const val KEYSTORE_ALIAS = "com.xiwei.sujian.secure_storage_v1"
private const val KEYSTORE_TYPE = "AndroidKeyStore"
private const val SECRETS_DIR = "keystore_secrets"
private const val TRANSFORMATION = "AES/GCM/NoPadding"
private const val GCM_IV_LENGTH = 12
private const val GCM_TAG_LENGTH = 128
private const val OLD_KEY_FILE = ".enc_key"
private const val OLD_SECRETS_DIR = "secrets"

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

    override fun getSecret(key: String): ByteArray? {
        try {
            val file = File(secretsDir, "$key.bin")
            if (!file.exists()) return null
            val data = file.readBytes()
            if (data.size < GCM_IV_LENGTH) {
                DiagnosticsLogger.w(TAG, "Truncated ciphertext for key=$key")
                return null
            }
            val iv = data.copyOfRange(0, GCM_IV_LENGTH)
            val ciphertext = data.copyOfRange(GCM_IV_LENGTH, data.size)
            val secretKey = getKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            val spec = GCMParameterSpec(GCM_TAG_LENGTH, iv)
            cipher.init(Cipher.DECRYPT_MODE, secretKey, spec)
            return cipher.doFinal(ciphertext)
        } catch (e: UnrecoverableKeyException) {
            DiagnosticsLogger.e(TAG, "Keystore key invalidated for key=$key", e)
            throw SecureStorageException.KeystoreKeyInvalidated()
        } catch (e: CertificateException) {
            DiagnosticsLogger.e(TAG, "Keystore certificate error for key=$key", e)
            throw SecureStorageException.KeystoreException()
        } catch (e: SecureStorageException) {
            throw e
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to get secret for key=$key", e)
            throw SecureStorageException.StorageException()
        }
    }

    override fun setSecret(key: String, value: ByteArray) {
        try {
            val secretKey = getKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, secretKey)
            val iv = cipher.iv
            val ciphertext = cipher.doFinal(value)
            val output = ByteArray(iv.size + ciphertext.size)
            System.arraycopy(iv, 0, output, 0, iv.size)
            System.arraycopy(ciphertext, 0, output, iv.size, ciphertext.size)
            if (!secretsDir.exists()) secretsDir.mkdirs()
            atomicWrite(File(secretsDir, "$key.bin"), output)
        } catch (e: UnrecoverableKeyException) {
            DiagnosticsLogger.e(TAG, "Keystore key invalidated for key=$key", e)
            throw SecureStorageException.KeystoreKeyInvalidated()
        } catch (e: SecureStorageException) {
            throw e
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to set secret for key=$key", e)
            throw SecureStorageException.StorageException()
        }
    }

    override fun deleteSecret(key: String) {
        try {
            val file = File(secretsDir, "$key.bin")
            if (file.exists()) {
                file.delete()
            }
        } catch (e: SecureStorageException) {
            throw e
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to delete secret for key=$key", e)
            throw SecureStorageException.StorageException()
        }
    }

    private fun atomicWrite(target: File, data: ByteArray) {
        val tempFile = File(target.parent, "${target.name}.tmp")
        try {
            FileOutputStream(tempFile).use { fos ->
                fos.write(data)
                fos.fd.sync()
            }
            if (tempFile.renameTo(target)) {
                return
            }
            target.writeBytes(data)
        } finally {
            tempFile.delete()
        }
    }

    var migrationError: String? = null
        private set

    private fun migrateOldEncKey() {
        migrationError = migrateOldEncKeyInternal()
        if (migrationError != null) {
            DiagnosticsLogger.e(TAG, "Keystore migration failed (Keystore still usable for new secrets): $migrationError")
        }
    }

    private fun migrateOldEncKeyInternal(): String? {
        val oldKeyFile = File(context.noBackupFilesDir, OLD_KEY_FILE)
        if (!oldKeyFile.exists()) return null

        val oldSecretsDir = File(context.noBackupFilesDir, OLD_SECRETS_DIR)
        if (!oldSecretsDir.exists() || !oldSecretsDir.isDirectory) {
            oldKeyFile.delete()
            DiagnosticsLogger.i(TAG, "Migrated old .enc_key: no old secrets dir, deleted key file")
            return null
        }

        val encFiles = oldSecretsDir.listFiles()?.filter { it.extension == "enc" } ?: emptyList()
        if (encFiles.isEmpty()) {
            oldSecretsDir.deleteRecursively()
            oldKeyFile.delete()
            DiagnosticsLogger.i(TAG, "Migrated old .enc_key: no .enc files, cleaned up")
            return null
        }

        val oldKeyData = try {
            oldKeyFile.readBytes()
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to read old .enc_key for migration", e)
            return "Failed to read old .enc_key: ${e.message}"
        }

        if (oldKeyData.size != 32) {
            DiagnosticsLogger.e(TAG, "Old .enc_key is not 32 bytes (got ${oldKeyData.size}), cannot migrate secrets. Old data preserved.")
            return "Old .enc_key is not 32 bytes (got ${oldKeyData.size})"
        }

        val oldAesKey = SecretKeySpec(oldKeyData, "AES")

        val setSecretNames = mutableListOf<String>()
        var failedCount = 0

        for (encFile in encFiles) {
            try {
                val secretName = encFile.nameWithoutExtension
                val encData = encFile.readBytes()
                if (encData.size < GCM_IV_LENGTH) {
                    DiagnosticsLogger.e(TAG, "Old .enc file ${encFile.name} too short, skipping")
                    failedCount++
                    continue
                }
                val nonce = encData.copyOfRange(0, GCM_IV_LENGTH)
                val ciphertext = encData.copyOfRange(GCM_IV_LENGTH, encData.size)

                val cipher = Cipher.getInstance(TRANSFORMATION)
                val spec = GCMParameterSpec(GCM_TAG_LENGTH, nonce)
                cipher.init(Cipher.DECRYPT_MODE, oldAesKey, spec)
                val plaintext = cipher.doFinal(ciphertext)

                setSecret(secretName, plaintext)
                setSecretNames.add(secretName)

                val readBack = getSecret(secretName)
                if (readBack == null || !readBack.contentEquals(plaintext)) {
                    DiagnosticsLogger.e(TAG, "Migration verification failed for secret $secretName: read-back mismatch")
                    failedCount++
                    continue
                }
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "Failed to migrate secret ${encFile.name}", e)
                failedCount++
            }
        }

        if (failedCount == 0) {
            oldSecretsDir.deleteRecursively()
            oldKeyFile.delete()
            DiagnosticsLogger.i(TAG, "Migrated ${setSecretNames.size} old secrets to Keystore successfully")
            return null
        } else {
            for (name in setSecretNames) {
                try {
                    deleteSecret(name)
                } catch (e: Exception) {
                    DiagnosticsLogger.e(TAG, "Failed to roll back migrated secret $name", e)
                }
            }
            DiagnosticsLogger.e(TAG, "Migration failed: ${setSecretNames.size} rolled back, $failedCount failed. All old data preserved.")
            return "Migration failed: $failedCount secrets could not be migrated, ${setSecretNames.size} rolled back"
        }
    }
}
