package com.xiwei.sujian.data

import java.util.concurrent.atomic.AtomicBoolean

/**
 * SyncChangeBus — 同步状态变更事件总线
 *
 * 使用 AtomicBoolean 实现线程安全的同步状态变更通知机制。
 *
 * ## 架构定位
 * - 跨组件通知同步状态变更
 * - 消费者模式：消费后自动重置状态
 *
 * ## 使用场景
 * - 同步完成后通知 UI 刷新
 * - SettingsActivity 监听同步状态变化
 */
object SyncChangeBus {
    private val changed = AtomicBoolean(false)

    fun notifyChanged() {
        changed.set(true)
    }

    fun consumeChanged(): Boolean {
        return changed.getAndSet(false)
    }
}
