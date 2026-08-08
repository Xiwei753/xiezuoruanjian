package com.xiwei.sujian.core.interop.sync

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * #592 四/六：进程级同步配置串行边界（suspend 版）。
 *
 * commitSyncProfile（stagedConfig → stagedSecrets → activeGeneration 提交）与
 * snapshotSyncProfile（正式同步/自动同步/试运行/连接诊断读取完整快照）以及
 * 同步启动共用同一把进程级 Mutex，消除设置提交与自动同步读取之间的竞态。
 *
 * - commitExclusive：设置页提交同步配置时持有锁，保证整个 staging + 原子提交序列
 *   对 snapshot 不可见；未提交的 staged 状态不会被读取者当作完整版本。
 * - snapshotExclusive：读取 config + secrets 时持有锁，保证读到完整提交的
 *   同一组数据，不会读到半提交状态。
 *
 * 锁本身只协调访问时序，不维护第二份业务真相；config/secrets 仍由 Core 层唯一持有，
 * DataStore 只保存 generation 提交标记与恢复载荷。
 */
object SyncProfileGate {
    private val mutex = Mutex()

    suspend fun <T> commitExclusive(block: suspend () -> T): T = mutex.withLock { block() }

    suspend fun <T> snapshotExclusive(block: suspend () -> T): T = mutex.withLock { block() }
}
