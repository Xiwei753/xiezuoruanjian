package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 系统回调诊断事件 — Issue #671 第 3 部分。
 *
 * 涵盖应用/Activity 生命周期回调（origin=System）和系统主题变化回调（origin=System）。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object SystemDiagnosticsEvents {
    // ── 重复字段 key（同一文件内出现 2 次以上）──────────────────────

    private const val KEY_STATE = "state"

    // ── 应用生命周期（System：系统回调）──────────────────────────

    fun appLifecycle(state: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.SYSTEM, "app.lifecycle", KEY_STATE to state)

    fun activityLifecycle(state: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.SYSTEM, "app.activity", KEY_STATE to state)

    // ── 系统主题变化（System：系统回调）──────────────────────────

    /** 系统主题变化回调。origin=System。 */
    fun themeSystemColorScheme(isDark: Boolean) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.SYSTEM, "theme.system_color_scheme", "isDark" to isDark)
}
