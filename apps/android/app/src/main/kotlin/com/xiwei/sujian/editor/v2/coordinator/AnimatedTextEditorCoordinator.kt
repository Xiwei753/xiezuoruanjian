package com.xiwei.sujian.editor.v2.coordinator

/**
 * #592 一：此类已被拆分为 [EditorSessionCoordinator]（会话层）和 [EditorWindowHost]（窗口层）。
 * 共享类型 [EditorAnimationSettings] 和 [SessionResetSource] 已移至 [EditorCoordinatorTypes.kt]。
 *
 * EditorSessionViewModel 现在持有 EditorSessionCoordinator，不再持有窗口级对象。
 * 窗口宿主由 Compose 层按窗口生命周期创建和释放。
 */
