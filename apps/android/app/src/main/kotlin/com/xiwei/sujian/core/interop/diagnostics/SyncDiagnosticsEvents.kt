package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 同步诊断事件 — Issue #671 第 3 部分。
 *
 * origin 由调用点传入：
 * - 用户触发同步（Manual / SettingsPage / Import）→ User
 * - 定时自动同步（Auto / ForegroundService / scheduler）→ App
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object SyncDiagnosticsEvents {
    fun syncEvent(
        origin: DiagnosticOriginDto,
        action: String,
        status: String,
        detail: String? = null,
    ) = DiagnosticsEventsInterop.record(
        origin,
        "sync.event",
        "action" to action,
        "status" to status,
        "detail" to detail,
    )
}
