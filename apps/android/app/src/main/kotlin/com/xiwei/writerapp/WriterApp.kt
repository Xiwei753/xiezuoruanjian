package com.xiwei.writerapp

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
