package com.xiwei.sujian.core.interop.project

import java.util.concurrent.atomic.AtomicReference

/**
 * #600：进程级当前作品 id 桥接 — UI 层写入，数据层（同步）读取。
 *
 * Core sync API 已改为 per-project（Issue #600 删除 workspace 概念），
 * 同步操作需要知道目标作品 id。Android 的"当前作品"是 UI 层概念
 *（SujianAppViewModel.currentProjectId），后台同步（AutoSyncWorker）和
 * 设置页同步（SettingsViewModel）在数据层执行，无法直接访问 Compose 状态。
 *
 * 本对象与 ActiveDocumentGate 同构：进程级 AtomicReference 桥接，
 * UI 层是唯一写入方（单一事实来源），数据层只读。不复制同步状态机，
 * 不维护第二份业务真相。
 */
object ActiveProjectGate {
    private val projectIdRef = AtomicReference<String?>(null)

    /** UI 层设置当前作品 id（selectProject/clearProjectSelection 时调用）。 */
    fun setCurrentProjectId(projectId: String?) {
        projectIdRef.set(projectId)
    }

    /** 数据层读取当前作品 id；无活动作品时返回 null（调用方据此跳过同步）。 */
    fun currentProjectId(): String? = projectIdRef.get()
}
