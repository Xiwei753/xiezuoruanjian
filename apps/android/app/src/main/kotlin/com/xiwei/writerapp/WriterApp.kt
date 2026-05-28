package com.xiwei.writerapp

/**
 * WriterApp — Android 应用入口类
 *
 * 负责应用生命周期管理，监听前后台切换以触发自动同步。
 *
 * ## 架构定位
 * - 继承 Application，实现 LifecycleObserver
 * - 监听应用前后台切换事件，控制 AutoSyncScheduler 的启停
 *
 * ## 职责边界
 * - **做**：初始化应用、管理自动同步调度器的生命周期
 * - **不做**：业务逻辑（由 AutoSyncScheduler 和 Rust Core 负责）
 *
 * ## 依赖关系
 * - AutoSyncScheduler：自动同步调度器
 * - ProcessLifecycleOwner：Android 生命周期监听
 *
 * ## 使用场景
 * - 应用启动时自动初始化
 * - 应用进入前台时启动自动同步
 * - 应用进入后台时停止自动同步
 */

import android.app.Application
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleObserver
import androidx.lifecycle.OnLifecycleEvent
import androidx.lifecycle.ProcessLifecycleOwner
import com.xiwei.writerapp.data.AutoSyncScheduler

class WriterApp : Application(), LifecycleObserver {

    private var autoSyncScheduler: AutoSyncScheduler? = null

    override fun onCreate() {
        super.onCreate()
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
    }

    @OnLifecycleEvent(Lifecycle.Event.ON_START)
    fun onForeground() {
        if (autoSyncScheduler == null) {
            autoSyncScheduler = AutoSyncScheduler(this)
        }
        autoSyncScheduler?.start()
    }

    @OnLifecycleEvent(Lifecycle.Event.ON_STOP)
    fun onBackground() {
        autoSyncScheduler?.stop()
    }
}
