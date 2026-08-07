package com.xiwei.sujian.data

import java.util.concurrent.atomic.AtomicReference

/**
 * #595 三/四：工作区文档门 — 同步前统一 flush 活动正文 session 到 Repository。
 *
 * 手动同步、自动同步、试运行和连接诊断启动前都必须先让活动编辑器把
 * 未落盘的本地输入保存到磁盘，否则同步下载的新正文可能直接覆盖尚未保存的
 * 本地输入（Core 同步只能以磁盘内容为三方合并基础）。
 *
 * #595 四：注册携带 owner token（活动 EditorViewModel 实例）。旧实例的
 * [Registration.close] 只在自己仍是当前持有者时注销 — Activity 重建、
 * 多窗口或生命周期交错时，旧 ViewModel 的 onCleared 不得清掉新实例的注册
 * （旧实现是进程级 AtomicReference 裸覆盖：旧实例销毁会把新实例的 flusher
 * 清掉，之后同步会认为"没有活动编辑器"直接继续执行）。
 *
 * #595 三：同步全过程持有文档身份 lease — flush 后取一次文档身份，
 * 同步完成后校验是否仍匹配。章节切换/关闭导致身份变化时不应用同步结果，
 * 新输入作为下一代 dirty 文档继续保存。
 *
 * 本对象只持有协作回调，不复制同步状态机，不维护第二份业务真相。
 */
object WorkspaceDocumentGate {
    private class Holder(
        val owner: Any,
        val flush: suspend () -> Boolean,
        val documentIdentity: () -> String?,
    )

    private val holder = AtomicReference<Holder?>(null)

    /**
     * 注册活动正文 flush 回调（EditorViewModel.initialize 时调用）。
     * 返回的 [Registration] 必须由同一实例在 onCleared 时 close。
     * 同 owner 重复注册替换当前回调，但旧 [Registration] 只清除自己的 Holder。
     *
     * [documentIdentity] 返回当前活动文档的身份标识（targetId:sessionId:epoch），
     * 同步前后校验文档身份是否变化。
     */
    fun register(
        owner: Any,
        flush: suspend () -> Boolean,
        documentIdentity: () -> String? = { null },
    ): Registration {
        val h = Holder(owner, flush, documentIdentity)
        holder.set(h)
        return Registration(h)
    }

    /**
     * 注册句柄 — close 只清除自己创建的那个 Holder。
     * 旧 ViewModel 的 onCleared 不得清除新实例（或同 owner 新注册）的回调。
     */
    class Registration internal constructor(private val myHolder: Any?) {
        fun close() {
            holder.compareAndSet(myHolder as? Holder, null)
        }
    }

    /**
     * flush 活动正文到 Repository。无注册回调（无活动编辑器）时返回 true —
     * 没有本地输入需要保护。
     */
    suspend fun flushActiveDocument(): Boolean {
        val h = holder.get() ?: return true
        return try {
            h.flush() ?: true
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Exception) {
            false
        }
    }

    /**
     * #595 三：当前活动文档身份 — 同步前后校验文档是否仍是同一 target/session/epoch。
     * 无注册回调时返回 null（无活动编辑器，不需要校验）。
     */
    fun activeDocumentIdentity(): String? = holder.get()?.documentIdentity()
}
