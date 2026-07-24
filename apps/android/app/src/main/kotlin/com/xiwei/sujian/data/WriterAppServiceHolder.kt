package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException
import uniffi.writer_core.openWorkspaceWithInit

/**
 * 底层 WriterAppService 持有者 + 公共错误包装能力。
 *
 * 所有领域 Bridge 通过此类获取 service 实例和统一的 wrapResult 错误处理。
 *
 * 服务通过 [openWorkspaceWithInit] 创建，注入平台初始化参数（目录、设备 ID、
 * 网络状态等），Core 不再自行猜测平台目录。
 */
class WriterAppServiceHolder(
    workspacePath: String,
    platformInit: PlatformInitDto? = null,
) {
    val service: WriterAppService by lazy {
        if (platformInit != null) {
            openWorkspaceWithInit(workspacePath, platformInit)
        } else {
            WriterAppService(workspacePath)
        }
    }

    companion object {
        private const val TAG = "WriterAppServiceHolder"

        fun createFromContext(
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
            return WriterAppServiceHolder(workspacePath, init)
        }
    }

    /**
     * 统一错误包装：将 Core 调用结果包装为 [BridgeResult]。
     *
     * - [UnsatisfiedLinkError] → [BridgeResult.NotLoaded]（原生库未加载）
     * - [WriterException] → [BridgeResult.Error]（Core 错误，errorCode 来自 UniFFI）
     * - 其他 [Exception] → [BridgeResult.Error]（errorCode = UNKNOWN）
     */
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
