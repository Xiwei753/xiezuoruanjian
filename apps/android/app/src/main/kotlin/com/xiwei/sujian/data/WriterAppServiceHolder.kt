package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.platform.AndroidKeystoreSecureStorage
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException
import uniffi.writer_core.openWorkspaceWithSecureStorage
import uniffi.writer_core.openWorkspaceWithInit

class WriterAppServiceHolder(
    workspacePath: String,
    platformInit: PlatformInitDto? = null,
    secureStorageProvider: uniffi.writer_core.SecureStorageProvider? = null,
    val secureStorageError: String? = null,
    private val keystoreStorage: com.xiwei.sujian.platform.AndroidKeystoreSecureStorage? = null,
) {
    private val serviceLazy = lazy {
        if (platformInit != null && secureStorageProvider != null) {
            openWorkspaceWithSecureStorage(workspacePath, platformInit, secureStorageProvider)
        } else if (platformInit != null) {
            openWorkspaceWithInit(workspacePath, platformInit)
        } else {
            WriterAppService(workspacePath)
        }
    }
    val service: WriterAppService by serviceLazy

    fun close() {
        if (serviceLazy.isInitialized()) {
            service.close()
        }
    }

    @Volatile
    private var _migrationWarningDismissed = false

    val secureStorageWarning: String?
        get() {
            val error = secureStorageError ?: return null
            if (_migrationWarningDismissed) return null
            return if (error.startsWith("migration_failed:")) error else null
        }

    fun dismissMigrationWarning() {
        _migrationWarningDismissed = true
        keystoreStorage?.markMigrationCompleted()
    }

    companion object {
        private const val TAG = "WriterAppServiceHolder"

        fun createFromContext(
            context: Context,
            workspacePath: String,
            filesDir: String,
            cacheDir: String,
            noBackupDir: String,
            deviceId: String,
            appVersion: String,
            locale: String,
            timezone: String,
            isConnected: Boolean,
            isMetered: Boolean,
        ): WriterAppServiceHolder {
            val init = PlatformInitDto(
                platform = PlatformDto.ANDROID,
                appDataDir = filesDir,
                cacheDir = cacheDir,
                logDir = "$cacheDir/log",
                noBackupDir = noBackupDir,
                deviceId = deviceId,
                appVersion = appVersion,
                locale = locale,
                timezone = timezone,
                isConnected = isConnected,
                isMetered = isMetered,
                proxyHost = null,
                proxyPort = null,
            )
            val secureStorage = try {
                AndroidKeystoreSecureStorage(context)
            } catch (e: Exception) {
                DiagnosticsLogger.e(TAG, "Failed to initialize Android Keystore secure storage", e)
                return WriterAppServiceHolder(workspacePath, init, null, secureStorageError = "keystore_init_failed")
            }
            if (secureStorage.migrationError != null) {
                return WriterAppServiceHolder(workspacePath, init, secureStorage, secureStorageError = "migration_failed:${secureStorage.migrationError}", keystoreStorage = secureStorage)
            }
            return WriterAppServiceHolder(workspacePath, init, secureStorage, keystoreStorage = secureStorage)
        }
    }

    fun <T> wrapResult(block: () -> T): BridgeResult<T> {
        return try {
            BridgeResult.Success(block())
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
    }
}
