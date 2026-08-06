package com.xiwei.sujian.data

import java.util.concurrent.atomic.AtomicReference

/**
 * #595 三：工作区文档门 — 同步前统一 flush 活动正文 session 到 Repository。
 *
 * 手动同步、自动同步、试运行和连接诊断启动前都必须先让活动编辑器把
 * 未落盘的本地输入保存到磁盘，否则同步下载的新正文可能直接覆盖尚未保存的
 * 本地输入（Core 同步只能以磁盘内容为三方合并基础）。
 *
 * 实现：活动 EditorViewModel 在 initialize 时注册 flush 回调（进程级单例，
 * 同一时刻只有一个活动正文 VM）；[SyncCoordinator.runSync] 在取得同步独占锁
 * 之前调用 [flushActiveDocument]：
 * - 返回 true：正文已保存（或没有活动编辑器），继续同步；
 * - 返回 false：flush 失败 — 同步必须中止（类型化失败），避免覆盖本地输入。
 *
 * 本对象只持有协作回调，不复制同步状态机，不维护第二份业务真相。
 */
object WorkspaceDocumentGate {
    private val flusher = AtomicReference<suspend () -> Boolean?>(null)

    /**
     * 注册活动正文 flush 回调（EditorViewModel.initialize 时调用；
     * onCleared 时用 null 注销）。同一进程同一时刻只有一个活动正文 VM。
     */
    fun registerFlusher(flush: (suspend () -> Boolean)?) {
        flusher.set(flush)
    }

    /**
     * flush 活动正文到 Repository。无注册回调（无活动编辑器）时返回 true —
     * 没有本地输入需要保护。
     */
    suspend fun flushActiveDocument(): Boolean {
        val f = flusher.get() ?: return true
        return try {
            f() ?: true
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Exception) {
            false
        }
    }
}
