package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 应用自身生命周期/内部状态诊断事件 — Issue #671 第 3 部分。
 *
 * 涵盖构建身份、进程启动、作品/章节加载保存、编辑器会话生命周期、
 * 主题解析，以及一级导航、工作区选择、主题外观选择等应用级用户操作事件。
 *
 * 编辑器运行时事件（动画事务、运动/排版策略、视口/重基接线）已拆到
 * [EditorRuntimeDiagnosticsEvents]，以控制本对象函数数。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object AppDiagnosticsEvents {
    // ── 重复字段 key（同一文件内出现 2 次以上）──────────────────────

    private const val KEY_PROJECT_ID = "projectId"
    private const val KEY_CHAPTER_ID = "chapterId"
    private const val KEY_BYTES = "bytes"
    private const val KEY_RESULT = "result"
    private const val KEY_ELAPSED_MS = "elapsedMs"
    private const val KEY_TARGET = "target"

    // ── 构建身份 / 进程启动（App：应用自身）──────────────────────

    fun appBuild(
        versionName: String,
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        applicationId: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "app.build",
        "versionName" to versionName,
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "applicationId" to applicationId,
    )

    fun appProcessStart(
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        processStartMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "app.process_start",
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "processStartMs" to processStartMs,
    )

    // ── 作品/卷/章节（App：应用加载保存）──────────────────────────

    fun chapterLoad(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "chapter.load",
        KEY_PROJECT_ID to projectId,
        KEY_CHAPTER_ID to chapterId,
        KEY_BYTES to byteLength,
        KEY_RESULT to result,
    )

    fun chapterSave(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
        elapsedMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "chapter.save",
        KEY_PROJECT_ID to projectId,
        KEY_CHAPTER_ID to chapterId,
        KEY_BYTES to byteLength,
        KEY_RESULT to result,
        KEY_ELAPSED_MS to elapsedMs,
    )

    // ── 编辑器会话生命周期（App：应用内部）────────────────────────

    fun sessionLifecycle(
        sessionId: String,
        action: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.session",
        "session" to sessionId,
        "action" to action,
    )

    // ── 主题解析（App：应用内部）──────────────────────────────────

    fun themeResolve(
        appearanceMode: String,
        colorSource: String,
        isDark: Boolean,
        selectedBuiltin: String?,
        selectedPalette: String?,
        sdk: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "theme.resolve",
        "appearanceMode" to appearanceMode,
        "colorSource" to colorSource,
        "isDark" to isDark,
        "selectedBuiltin" to selectedBuiltin,
        "selectedPalette" to selectedPalette,
        "sdk" to sdk,
    )

    // ── 一级导航（User：用户点击导航）────────────────────────────

    fun navigation(destination: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.destination", "destination" to destination)

    fun navBack(handled: Boolean) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.USER,
            "nav.back",
            "handled" to handled,
        )

    fun workspaceBack(target: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.workspace_back", KEY_TARGET to target)

    fun predictiveBack(
        target: String,
        phase: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "nav.predictive_back",
        KEY_TARGET to target,
        "phase" to phase,
    )

    fun navTopLevelSwitch(
        from: String,
        to: String,
    ) = DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.top_level_switch", "from" to from, "to" to to)

    // ── 作品/卷/章节选择（User：用户选择工作区）──────────────────

    fun workspaceSelection(
        kind: String,
        id: String,
    ) = DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "workspace.select", "kind" to kind, "id" to id)

    fun workspaceClear(kind: String) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.USER,
            "workspace.clear",
            "kind" to kind,
        )

    // ── 主题外观选择（User：用户选择外观模式）──────────────────────

    /** 用户选择外观模式（点击 dark/light/system）。origin=User。 */
    fun themeAppearanceSelect(requested: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "theme.appearance_select", "requested" to requested)

    // ── 主题 Material 颜色诊断（App：应用内部）──────────────────────

    /**
     * 最终塞进 MaterialTheme 的颜色低频诊断 — Issue #698 评论 5697617362。
     *
     * theme.resolve 只证明设置层选择了什么；本事件记录最终 colorScheme 的关键颜色，
     * 用于真机判断"设置层正确但 MaterialTheme 还是旧颜色"还是"MaterialTheme 已换但页面写死颜色"。
     * 调用方（SujianTheme）只在关键 key 变化时记录，保证低频。
     */
    fun themeMaterialColors(
        source: String,
        isDark: Boolean,
        colors: ThemeMaterialColorSnapshot,
        revision: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "theme.material_colors",
        "source" to source,
        "isDark" to isDark,
        "primary" to colors.primary,
        "primaryContainer" to colors.primaryContainer,
        "surface" to colors.surface,
        "surfaceContainer" to colors.surfaceContainer,
        "onSurface" to colors.onSurface,
        "revision" to revision,
    )
}

/**
 * 最终塞进 MaterialTheme 的关键颜色快照 — Issue #698 评论 5697617362 诊断用。
 *
 * 只记录关键颜色，用于真机判断 MaterialTheme 是否真的换了颜色。
 * 构造函数阈值 10，容纳 5 个颜色字段无需 @Suppress。
 */
data class ThemeMaterialColorSnapshot(
    val primary: String,
    val primaryContainer: String,
    val surface: String,
    val surfaceContainer: String,
    val onSurface: String,
)
