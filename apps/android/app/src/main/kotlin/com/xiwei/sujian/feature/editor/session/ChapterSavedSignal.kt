package com.xiwei.sujian.feature.editor.session

/**
 * #625 项6：章节保存成功信号 — 章节正文真正落盘（[EditorSessionCoordinator.commitSavedLease]
 * 校验通过、localDirty 清除）后由 EditorViewModel emit，app 层收集后刷新作品摘要列表
 * （含字数/卷数/章节数）。
 *
 * 这是纯事件触发信号，只携带章节标识，不含字数/摘要等业务数据 — 不构成第二数据源，
 * 也不镜像 Core 状态。app 层收到信号后全量 [com.xiwei.sujian.app.SujianAppViewModel.refreshProjectSummaries]，
 * 不依赖此信号的字段值（仅用于诊断/未来精确刷新）。
 *
 * 信号经 [EditorSessionCoordinator] 中转：EditorViewModel 与 app 层创建的
 * [com.xiwei.sujian.feature.editor.window.EditorWindowHost] 共享同一 sessionCoordinator，
 * editor feature 层不依赖 app 层。
 */
data class ChapterSavedSignal(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
)
