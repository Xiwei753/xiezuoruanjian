package com.xiwei.sujian.core.interop.diagnostics

import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import uniffi.writer_core.DiagnosticLevelDto
import uniffi.writer_core.DiagnosticOriginDto

/**
 * 统一脱敏诊断事件来源 — Issue #670 评论 5651060802 / 5651816143。
 *
 * 替代旧的 `core.diagnostics.DiagnosticsEvents`，底层通过
 * [DiagnosticsInterop.recordEvent] 转发到 Rust `recordDiagnosticEvent`，
 * 不再自己定义日志后端或调用 `DiagnosticsLogger.i`。
 *
 * 每个事件只记录事件类型、长度、字节范围、标识、状态码与耗时；
 * 严禁记录正文、preedit 内容、token 和密钥（RingBuffer 与 Rust redact 双重脱敏兜底）。
 *
 * RingBuffer 由生产代码持续写入，导出时同时包含内存事件快照与滚动日志文件。
 *
 * ## 事件来源（origin）— Issue #670 评论 5651060802 第 7 节
 *
 * `origin` 由真正知道触发源的调用点传入，后端不猜：
 * - **User**：用户主动操作（点击、选择、输入、手动触发同步）
 * - **System**：系统回调/环境变化（生命周期、网络变化、系统主题变化、崩溃）
 * - **App**：应用自身生命周期/内部状态（设置保存、主题解析、自动同步）
 *
 * Issue #670 评论 5651816143 修改 5：普通结构化业务事件默认传 INFO level。
 *
 * ## 按领域拆分 — Issue #671 第 3 部分
 *
 * 40 个事件函数已按职责拆到五个领域 object，本 object 只保留公共 [record]：
 * - [AppDiagnosticsEvents]：应用自身生命周期/内部状态、导航、工作区选择、主题外观。
 * - [EditorDiagnosticsEvents]：编辑器用户输入（焦点、composition、编辑事务、字段）。
 * - [SettingsDiagnosticsEvents]：设置保存与分区展开/折叠。
 * - [SyncDiagnosticsEvents]：同步事件。
 * - [SystemDiagnosticsEvents]：系统回调（生命周期、系统主题变化）。
 */
object DiagnosticsEventsInterop {
    private const val TAG = "SujianDiag"

    fun record(
        origin: DiagnosticOriginDto,
        eventType: String,
        vararg fields: Pair<String, Any?>,
    ) {
        val event =
            linkedMapOf<String, Any?>("event" to eventType).apply {
                for ((k, v) in fields) {
                    if (v != null) put(k, v)
                }
                put("ts", System.currentTimeMillis())
            }
        EditorEventRingBuffer.record(event)
        // 转发到 Rust 统一诊断后端（脱敏 + JSONL 持久化）。
        // 修改 5：普通结构化业务事件默认传 INFO level。
        val dtos =
            fields.mapNotNull { (k, v) ->
                if (v == null) null else uniffi.writer_core.DiagnosticFieldDto(k, v.toString())
            }
        DiagnosticsInterop.recordEvent(
            DiagnosticLevelDto.INFO,
            origin,
            eventType,
            TAG,
            null,
            dtos,
        )
    }
}
