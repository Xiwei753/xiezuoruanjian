package com.xiwei.sujian.core.interop.app
import android.content.Context
import com.xiwei.sujian.app.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.core.interop.common.toSyncFailureKind
import com.xiwei.sujian.core.interop.common.toWireErrorCode
import com.xiwei.sujian.core.interop.sync.AndroidKeystoreSecureStorage
import com.xiwei.sujian.core.platform.AndroidDataRoot
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException
import uniffi.writer_core.openAppServiceWithInit
import uniffi.writer_core.openAppServiceWithSecureStorage

class WriterAppServiceHolder(
    appDataRoot: String,
    projectsRoot: String,
    platformInit: PlatformInitDto? = null,
    secureStorageProvider: uniffi.writer_core.SecureStorageProvider? = null,
    val secureStorageError: String? = null,
    private val keystoreStorage: com.xiwei.sujian.core.interop.sync.AndroidKeystoreSecureStorage? = null,
) {
    @Volatile
    private var _initError: WriterException? = null

    private val serviceLazy =
        lazy {
            try {
                if (platformInit != null && secureStorageProvider != null) {
                    openAppServiceWithSecureStorage(appDataRoot, projectsRoot, platformInit, secureStorageProvider)
                } else if (platformInit != null) {
                    openAppServiceWithInit(appDataRoot, projectsRoot, platformInit)
                } else {
                    WriterAppService(appDataRoot, projectsRoot)
                }
            } catch (e: WriterException) {
                DiagnosticsLogger.e(TAG, "Failed to open app service: ${e.message}", e)
                _initError = e
                throw e
            }
        }
    val service: WriterAppService by serviceLazy

    val initError: WriterException?
        get() = _initError

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
            cacheDir: String,
            noBackupDir: String,
            deviceId: String,
            appVersion: String,
            locale: String,
            timezone: String,
            isConnected: Boolean,
            isMetered: Boolean,
        ): WriterAppServiceHolder {
            val appDataRoot = AndroidDataRoot.rootDir().absolutePath
            val projectsRoot = AndroidDataRoot.projectsDir().absolutePath
            val init =
                PlatformInitDto(
                    platform = PlatformDto.ANDROID,
                    appDataDir = appDataRoot,
                    cacheDir = cacheDir,
                    logDir = AndroidDataRoot.logsDir().absolutePath,
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
            val secureStorage =
                try {
                    AndroidKeystoreSecureStorage(context)
                } catch (e: Exception) {
                    DiagnosticsLogger.e(TAG, "Failed to initialize Android Keystore secure storage", e)
                    return WriterAppServiceHolder(
                        appDataRoot,
                        projectsRoot,
                        init,
                        null,
                        secureStorageError = "keystore_init_failed",
                    )
                }
            if (secureStorage.migrationError != null) {
                return WriterAppServiceHolder(
                    appDataRoot,
                    projectsRoot,
                    init,
                    secureStorage,
                    secureStorageError = "migration_failed:${secureStorage.migrationError}",
                    keystoreStorage = secureStorage,
                )
            }
            return WriterAppServiceHolder(
                appDataRoot,
                projectsRoot,
                init,
                secureStorage,
                keystoreStorage = secureStorage,
            )
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
            // #592 七：类型化失败在 Bridge 边界由 WriterException 变体直接推导。
            BridgeResult.Error(
                ResultEnvelope.errorOf(e.toWireErrorCode(), e.message ?: "Unknown native exception"),
                syncFailureKind = e.toSyncFailureKind(),
            )
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.errorOf("UNKNOWN", e.message ?: "Unknown error"))
        }
    }
}
