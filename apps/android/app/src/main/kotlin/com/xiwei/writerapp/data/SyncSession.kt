package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/**
 * SyncSession — 同步会话状态管理
 *
 * 使用原子操作管理同步任务的并发锁和任务 ID。
 *
 * ## 架构定位
 * - 全局单例，管理同步任务的并发访问
 * - 防止多个同步任务同时执行
 *
 * ## 使用场景
 * - SettingsActivity 中的同步按钮状态管理
 * - 防止重复点击导致的并发同步问题
 */
object SyncSession {
    val lock = AtomicBoolean(false)
    val currentTaskId = AtomicInteger(0)
}
