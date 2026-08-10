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
 * 线程安全模型：[lock] 守护 [pending]、[swap] 与两个计数器；文件 I/O 在
 * lock 外只由 writer 线程执行，故 enqueue 与 flushBlocking 不会阻塞 I/O。
 * 所有通知均用 notifyAll：enqueue 的通知不能只唤醒 flushBlocking 而漏掉
 * writer，否则 writer 永远不被唤醒处理 pending。
 */
internal object PersistentLogWriter {
    private const val LOG_PREFIX = "sujian-current"
    private const val MAX_FILE_SIZE = 1024 * 1024L // 1 MiB
    private const val MAX_LOG_FILES = 5

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
     */
    fun flushBlocking() {
        synchronized(lock) {
            if (!initialized) return
            val target = enqueuedCount
            while (processedCount < target) {
                // writer 完成每个 batch 后 notifyAll 唤醒本等待；
                // enqueue 的 notifyAll 保证 writer 不会漏掉 pending。
                lock.wait()
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
     * 正在 writer 线程执行的 batch 不会被中断（已交换出 lock），写完后
     * 文件句柄关闭，Linux 内核回收已删除 inode。
     */
    fun clearLogs() {
        synchronized(lock) {
            pending.clear()
            swap.clear()
            // 让可能正在等待的 flushBlocking 立即返回：清空后不会再有新进度。
            enqueuedCount = processedCount
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
                    lock.wait()
                }
                // 交换 pending 与 swap：writer 接手 swap（原 pending），
                // enqueue 后续写入新的 pending（原 swap）。
                batch = pending
                pending = swap
                swap = batch
                batchSize = batch.size
            }
            writeBatch(batch)
            synchronized(lock) {
                batch.clear()
                processedCount += batchSize
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
        } catch (_: Exception) {
            // I/O 失败：丢弃本批次，避免 writer 线程被磁盘异常反复阻塞。
            // 不向上抛：writer 线程不能死。
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
