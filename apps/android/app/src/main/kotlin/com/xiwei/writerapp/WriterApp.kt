package com.xiwei.writerapp

/**
 * WriterApp — Android 应用入口类
 *
 * 负责应用生命周期管理，监听前台切换以补充触发自动同步。
 *
 * ## 架构定位
 * - 继承 Application，实现 LifecycleObserver
 * - 监听应用前台切换事件，触发 AutoSyncScheduler 配置 WorkManager
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
 * - 应用进入前台时调度一次即时检查
 * - 周期自动同步由 WorkManager 持久调度，退后台后不取消
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
