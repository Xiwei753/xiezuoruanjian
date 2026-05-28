package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean

/**
 * SettingsChangeBus — 设置变更事件总线
 *
 * 使用 AtomicBoolean 实现线程安全的设置变更通知机制。
 *
 * ## 架构定位
 * - 跨组件通知设置变更
 * - 支持通用设置变更和编辑器设置变更的分别消费
 *
 * ## 使用场景
 * - SettingsActivity 保存设置后通知其他页面刷新
 * - EditorActivity 监听编辑器设置变更
 * - 消费者模式：消费后自动重置状态
 */
object SettingsChangeBus {
    private val changed = AtomicBoolean(false)
    private val editorChanged = AtomicBoolean(false)

    fun notifyChanged() {
        changed.set(true)
        editorChanged.set(true)
    }

    fun markChanged() {
        notifyChanged()
    }

    fun consumeChanged(): Boolean {
        return changed.getAndSet(false)
    }

    fun consumeEditorChanged(): Boolean {
        return editorChanged.getAndSet(false)
    }
}
