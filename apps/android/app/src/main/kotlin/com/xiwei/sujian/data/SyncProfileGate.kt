package com.xiwei.sujian.data

/**
 * #592 四：进程级同步配置串行边界。
 *
 * commitSyncProfile（暂存 config + secrets + 提交）与 snapshotSyncProfile
 * （读取正式同步使用的 config）共用同一把锁，消除设置提交与自动同步读取之间的竞态。
 *
 * - commitExclusive：设置页提交同步配置时持有锁，保证读旧 config → 存新 config → 存新 secrets
 *   整个序列对 snapshot 不可见。
 * - snapshotExclusive：正式同步、试运行、连接诊断读取 config 时持有锁，
 *   保证读到完整提交的同一组 config + secrets，不会读到半提交状态。
 *
 * 锁本身只协调访问时序，不维护第二份业务真相；config/secrets 仍由 Core 层唯一持有。
 */
object SyncProfileGate {
    private val lock = Any()

    fun <T> commitExclusive(block: () -> T): T = synchronized(lock, block)

    fun <T> snapshotExclusive(block: () -> T): T = synchronized(lock, block)
}
