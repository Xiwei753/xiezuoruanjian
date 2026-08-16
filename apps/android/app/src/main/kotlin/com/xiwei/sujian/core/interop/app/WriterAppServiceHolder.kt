package com.xiwei.sujian.core.interop.app
import android.content.Context
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.core.interop.common.toSyncFailureKind
import com.xiwei.sujian.core.interop.common.toWireErrorCode
import com.xiwei.sujian.core.interop.security.AndroidKeystoreSecureStorage
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
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
    private val keystoreStorage: com.xiwei.sujian.core.interop.security.AndroidKeystoreSecureStorage? = null,
) {
    @Volatile
    private var _initError: WriterException? = null

    private val serviceLazy =
        lazy {
            try {
                val svc =
                    if (platformInit != null && secureStorageProvider != null) {
                        openAppServiceWithSecureStorage(appDataRoot, projectsRoot, platformInit, secureStorageProvider)
                    } else if (platformInit != null) {
                        openAppServiceWithInit(appDataRoot, projectsRoot, platformInit)
                    } else {
                        WriterAppService(appDataRoot, projectsRoot)
                    }
                // #630 评论 5308439467 Part 1：冷启动恢复中断的 Syncing 状态。
                // 只有旧 full_state 是 Syncing 才原子改成 RecoverableError，
                // 避免进程被杀后顶部永久假黄灯。只在 service 创建时调用一次。
                // 失败只记日志，不阻断应用启动。
                try {
                    svc.recoverInterruptedFullSyncState()
                } catch (e: WriterException) {
                    DiagnosticsLogger.w(TAG, "Failed to recover interrupted full sync state: ${e.message}")
                }
                svc
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
