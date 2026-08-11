package com.xiwei.sujian.core.diagnostics

import android.content.Context
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * 单条日志写入请求。调用方在任意线程构造并 [enqueue]，所有文件 I/O
 * 只发生在常驻后台 writer 线程。
 */
internal data class LogRequest(
    val level: String,
    val tag: String,
    val message: String,
    val timestampMs: Long,
    val threadName: String,
)

/**
 * 常驻后台线程持久日志写入器（Signal Android PersistentLogger 路线）。
 *
 * - 单一 writer 线程 sujian-logger（[Thread.MIN_PRIORITY]）独占文件 I/O。
 * - 调用方 [enqueue] 只把 [LogRequest] 放进 pending 队列并 notifyAll()。
 * - writer 线程被唤醒后交换 pending/swap 双 buffer，把当前积累的全部请求
 *   一次性 append 到 AndroidDataRoot.logsDir()/sujian-current.log，每个
 *   batch 写完即 flush()。
 * - 1 MiB / 5 文件轮转，轮转、打开、append、flush 全部只在 writer 线程执行。
 *
 * 线程安全模型：[lock] 守护 [pending]、[swap] 与计数器；文件 I/O 在 lock 外
 * 只由 writer 线程执行，故 enqueue 与 flushBlocking 不会阻塞 I/O。
 * 所有通知均用 notifyAll：enqueue 的通知不能只唤醒 flushBlocking 而漏掉
 * writer，否则 writer 永远不被唤醒处理 pending。
 *
 * 中断语义：writer 是常驻 daemon 线程，[InterruptedException] 只当作一次
 * 虚假唤醒——不恢复中断位、继续等待，线程绝不退出（否则持久日志永久停写）。
 */
internal object PersistentLogWriter {
    private const val LOG_PREFIX = "sujian-current"
    private const val MAX_FILE_SIZE = 1024 * 1024L // 1 MiB
    private const val MAX_LOG_FILES = 5

    /**
     * flushBlocking / clearLogs 的等待上限：writer 因不可控 Error 死亡时，
     * 崩溃处理器与导出流程不能永久挂起（Issue #612 收口）。
     */
    private const val FLUSH_TIMEOUT_MS = 5_000L

    private val timestampFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)

    private val lock = java.lang.Object()
    private var pending = ArrayDeque<LogRequest>()
    private var swap = ArrayDeque<LogRequest>()

    @Volatile private var initialized = false

    @Volatile private var enabled = false

    // flushBlocking 同步信号：enqueuedCount 在 enqueue 时递增，
    // processedCount 在 writer 完成一个 batch 后按该 batch 大小递增。
    // flushBlocking 快照目标 enqueuedCount 后等到 processedCount 追上。
    private var enqueuedCount = 0L
    private var processedCount = 0L

    // 在途 batch 大小：writer 在 lock 内交换 batch 时置为 batchSize，
    // 写盘完成回到 lock 时清零。clearLogs 依赖它等待 writer 真正空闲，
    // 避免「文件已删、writer 随后 FileWriter(append) 重建并写回旧日志」竞态。
    private var inFlight = 0L

    // 清空代际：clearLogs 递增；flushBlocking 若发现代际变化立即返回——
    // 被清空丢弃的条目已不存在，等待它们落盘没有意义。
    private var clearGeneration = 0L

    /**
     * 初始化并启动 writer 线程。幂等：重复调用无副作用。
     * [context] 保留供未来绑定应用生命周期；当前日志目录由 [AndroidDataRoot] 决定。
     */
    fun init(context: Context) {
        synchronized(lock) {
            if (initialized) return
            initialized = true
            enabled = true
            val thread = Thread({ writerLoop() }, "sujian-logger")
            thread.priority = Thread.MIN_PRIORITY
            thread.isDaemon = true
            thread.start()
        }
        // 触发日志目录创建（在 lock 外，避免持有 lock 做 I/O）。
        ensureLogsDir()
    }

    fun setEnabled(enabled: Boolean) {
        synchronized(lock) {
            this.enabled = enabled
        }
    }

    /**
     * 把 [request] 放入 pending 队列。非阻塞：只 addLast + notifyAll。
     * 未初始化或被禁用时直接丢弃，不递增 enqueuedCount。
     */
    fun enqueue(request: LogRequest) {
        synchronized(lock) {
            if (!initialized || !enabled) return
            pending.addLast(request)
            enqueuedCount++
            lock.notifyAll()
        }
    }

    /**
     * 阻塞直到调用前所有已 enqueue 的请求都被 writer 写完落盘。
     * 语义：等待 pending queue 为空 + 当前正在写的 batch 完成。
     *
     * 若等待期间发生 [clearLogs]，被清空丢弃的条目永远无法落盘，此时按
     * 代际变化解除等待立即返回——那些日志已不存在，等待没有意义。
     * 最多等待 [FLUSH_TIMEOUT_MS]：writer 若因不可控 Error 死亡（磁盘/内存
     * 极端故障），调用方（崩溃处理器、导出）不能永久挂起。
     */
    fun flushBlocking() {
        synchronized(lock) {
            if (!initialized) return
            val target = enqueuedCount
            val generation = clearGeneration
            val deadline = System.nanoTime() + FLUSH_TIMEOUT_MS * 1_000_000L
            while (processedCount < target && clearGeneration == generation) {
                val remaining = deadline - System.nanoTime()
                if (remaining <= 0) return
                try {
                    lock.wait(remaining / 1_000_000L, (remaining % 1_000_000L).toInt())
                } catch (_: InterruptedException) {
                    // 调用方线程被中断：恢复中断位并返回，不做落盘保证。
                    Thread.currentThread().interrupt()
                    return
                }
            }
        }
    }

    /** 返回当前日志目录下所有 sujian-current*.log 文件。 */
    fun getLogFiles(): List<File> {
        val logDir = AndroidDataRoot.logsDir()
        if (!logDir.exists()) return emptyList()
        return logDir.listFiles { _, name -> name.startsWith(LOG_PREFIX) && name.endsWith(".log") }
            ?.toList() ?: emptyList()
    }

    /**
     * 清空 pending/swap 队列并删除日志目录下所有文件。
     *
     * 与 writer 并发安全：清空队列后先在 lock 内等待 [inFlight] 归零
     * （writer 完成正在写的 batch 回到 lock 时清零并 notifyAll），保证文件
     * 删除时 writer 已空闲，不会出现「文件已删、writer 随后重建文件并写回
     * 旧 batch」的复活竞态。
     */
    fun clearLogs() {
        synchronized(lock) {
            pending.clear()
            swap.clear()
            // 让可能正在等待的 flushBlocking 立即返回：清空后不再有可等待的进度。
            clearGeneration++
            enqueuedCount = processedCount
            // 等待 writer 完成正在写的 batch（写盘在 lock 外执行，完成后回到
            // lock 清零 inFlight 并 notifyAll 唤醒本等待）。最多等待
            // [FLUSH_TIMEOUT_MS]：writer 若因不可控 Error 死亡，清空不能永久挂起。
            val deadline = System.nanoTime() + FLUSH_TIMEOUT_MS * 1_000_000L
            while (inFlight > 0) {
                val remaining = deadline - System.nanoTime()
                if (remaining <= 0) break
                try {
                    lock.wait(remaining / 1_000_000L, (remaining % 1_000_000L).toInt())
                } catch (_: InterruptedException) {
                    // 清空语义要求文件删除前 writer 空闲；忽略中断继续等待，
                    // 不恢复中断位（否则 wait 会因中断位立即再抛，形成忙循环）。
                }
            }
            val logDir = AndroidDataRoot.logsDir()
            if (logDir.exists()) {
                logDir.listFiles()?.forEach { it.delete() }
            }
            lock.notifyAll()
        }
    }

    /** writer 线程主循环：wait → 交换双 buffer → 写盘 → 通知等待者。 */
    private fun writerLoop() {
        while (true) {
            val batch: ArrayDeque<LogRequest>
            val batchSize: Int
            synchronized(lock) {
                while (pending.isEmpty()) {
                    try {
                        lock.wait()
                    } catch (_: InterruptedException) {
                        // 常驻 daemon 线程：中断只当作虚假唤醒，不恢复中断位，
                        // 线程绝不退出。
                    }
                }
                // 交换 pending 与 swap：writer 接手 swap（原 pending），
                // enqueue 后续写入新的 pending（原 swap）。
                batch = pending
                pending = swap
                swap = batch
                batchSize = batch.size
                inFlight = batchSize.toLong()
            }
            writeBatch(batch)
            synchronized(lock) {
                batch.clear()
                processedCount += batchSize
                inFlight = 0L
                lock.notifyAll()
            }
        }
    }

    /** 把一整批请求 append 到当前日志文件，每个 batch 写完即 flush。 */
    private fun writeBatch(batch: ArrayDeque<LogRequest>) {
        if (batch.isEmpty()) return
        try {
            ensureLogsDir()
            val currentFile = File(AndroidDataRoot.logsDir(), "$LOG_PREFIX.log")
            rotateIfNeeded(currentFile)
            PrintWriter(FileWriter(currentFile, true)).use { writer ->
                for (req in batch) {
                    val ts = timestampFormat.format(Date(req.timestampMs))
                    writer.println("$ts ${req.level}/${req.tag}: ${req.message}")
                }
                writer.flush()
            }
        } catch (_: Throwable) {
            // 捕获 Throwable 而非 Exception：OutOfMemoryError 等 Error 逃逸会杀死
            // writer 线程，导致持久日志永久停写、flushBlocking 永久等待（诊断系统
            // 自身崩溃）。I/O 失败：丢弃本批次，writer 线程继续存活处理后续日志。
        }
    }

    /** 当前文件超过 1 MiB 时重命名为带时间戳的轮转文件，并裁剪到 5 个。 */
    private fun rotateIfNeeded(file: File) {
        if (!file.exists() || file.length() < MAX_FILE_SIZE) return
        val logDir = file.parentFile ?: return
        val rotated = File(logDir, "$LOG_PREFIX-${System.currentTimeMillis()}.log")
        file.renameTo(rotated)
        pruneOldLogs(logDir)
    }

    /** 按最后修改时间降序保留前 [MAX_LOG_FILES] 个，多余删除。 */
    private fun pruneOldLogs(logDir: File) {
        val logs =
            logDir.listFiles { _, name -> name.startsWith(LOG_PREFIX) && name.endsWith(".log") }
                ?.sortedByDescending { it.lastModified() }
                ?: return
        for (i in (MAX_LOG_FILES - 1) until logs.size) {
            logs[i].delete()
        }
    }

    private fun ensureLogsDir() {
        try {
            AndroidDataRoot.logsDir().mkdirs()
        } catch (_: Exception) {
            // 目录创建失败不阻断流程，writer 下次写入时重试。
        }
    }
}
