package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 设置诊断事件 — Issue #671 第 3 部分。
 *
 * 涵盖设置保存（origin=App）和设置分区展开/折叠（origin=User）。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object SettingsDiagnosticsEvents {
    // ── 设置（App：设置保存；User：用户展开/折叠分区）────────────

    fun settingsSaved(
        field: String,
        result: String,
    ) = DiagnosticsEventsInterop.record(DiagnosticOriginDto.APP, "settings.save", "field" to field, "result" to result)

    fun settingsSection(
        section: String,
        expanded: Boolean,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "settings.section",
        "section" to section,
        "expanded" to expanded,
    )
}
