package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException

/**
 * 底层 WriterAppService 持有者 + 公共错误包装能力。
 *
 * 所有领域 Bridge 通过此类获取 service 实例和统一的 wrapResult 错误处理。
 */
class WriterAppServiceHolder(workspacePath: String) {
    val service: WriterAppService by lazy { WriterAppService(workspacePath) }

    companion object {
        private const val TAG = "WriterAppServiceHolder"
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
