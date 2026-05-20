package com.xiwei.writerapp.data

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

object SyncSession {
    val lock = AtomicBoolean(false)
    val currentTaskId = AtomicInteger(0)
}
