package com.xiwei.sujian.core.diagnostics

import android.content.Context
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import java.io.File
import java.io.FileWriter
import java.io.PrintWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * 单条日志写入请求。调用方在任意线程构造并 [PersistentLogWriter.enqueue]，
 * 所有文件 I/O 只发生在常驻后台 writer 线程。
 */
internal data class LogRequest(
    val level: String,
    val tag: String,
    val message: String,
    val timestampMs: Long,
    val threadName: String,
)

/**
 * writer 线程处理的命令（Issue #612 评论 2）。单一有序队列保证：
 * 旧日志 → flush/clear barrier → 新日志 的全局顺序，无需计数器同步。
 */
internal sealed interface LogWriterCommand {
    /** 追加一条日志请求。 */
    data class Append(val request: LogRequest) : LogWriterCommand

    /** flush 屏障：writer 处理到此命令时先写完前序 Append，再 countDown 唤醒等待者。 */
    data class FlushBarrier(val latch: CountDownLatch) : LogWriterCommand

    /** clear 屏障：writer 处理到此命令时先写完前序 Append，删除日志目录所有文件，再 countDown。 */
    data class ClearBarrier(val latch: CountDownLatch) : LogWriterCommand
}

/**
 * 常驻后台线程持久日志写入器（Signal Android PersistentLogger 路线）。
 *
 * - 单一 writer 线程 sujian-logger（[Thread.MIN_PRIORITY]）独占文件 I/O。
 * - 调用方 [enqueue] 只把 [LogWriterCommand.Append] 入队并 notifyAll()。
 * - [flushBlocking] 入队 [LogWriterCommand.FlushBarrier] 并等待 latch；
 *   [clearLogs] 入队 [LogWriterCommand.ClearBarrier] 并等待 latch。
 * - writer 线程被唤醒后 drain 整个命令队列到本地列表，按入队顺序处理：
 *   连续 Append 收集为 batch 写盘，遇到屏障先写完 batch 再执行屏障语义。
 * - 1 MiB / 5 文件轮转，轮转、打开、append、flush、delete 全部只在 writer 线程执行。
 *
 * 线程安全模型：[lock] 守护 [queue]；文件 I/O 在 lock 外只由 writer 线程执行，
 * 故 enqueue 与 flushBlocking 不会阻塞 I/O。所有通知均用 notifyAll：
 * enqueue 的通知不能只唤醒 flushBlocking 而漏掉 writer，否则 writer 永远不被唤醒。
 *
 * 顺序不变量：barrier 入队前的所有 Append 先于 barrier 处理，barrier 后的 Append
 * 后于 barrier 处理——天然保证 flushBlocking 等到前序日志落盘、clearLogs 在 writer
 * 空闲后删除文件且后续 Append 不会写回旧日志。
 *
 * 中断语义：writer 是常驻 daemon 线程，[InterruptedException] 只当作一次虚假唤醒
 * ——不恢复中断位、继续等待，线程绝不退出（否则持久日志永久停写）。
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
    private val queue = ArrayDeque<LogWriterCommand>()

    @Volatile private var initialized = false

    @Volatile private var enabled = false

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
     * 把 [request] 入队为 Append 命令。非阻塞：只 addLast + notifyAll。
     * 未初始化或被禁用时直接丢弃。
     */
    fun enqueue(request: LogRequest) {
        synchronized(lock) {
            if (!initialized || !enabled) return
            queue.addLast(LogWriterCommand.Append(request))
            lock.notifyAll()
        }
    }

    /**
     * 阻塞直到调用前所有已 enqueue 的请求都被 writer 写完落盘。
     *
     * 实现：入队 FlushBarrier，writer 处理到此屏障时已写完前序所有 Append，
     * 然后 countDown 唤醒本等待。最多等待 [FLUSH_TIMEOUT_MS]：writer 若因不可控
     * Error 死亡（磁盘/内存极端故障），调用方（崩溃处理器、导出）不能永久挂起。
     * 未初始化时直接返回。
     */
    fun flushBlocking() {
        val latch = CountDownLatch(1)
        synchronized(lock) {
            if (!initialized) return
            queue.addLast(LogWriterCommand.FlushBarrier(latch))
            lock.notifyAll()
        }
        try {
            latch.await(FLUSH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (_: InterruptedException) {
            // 调用方线程被中断：恢复中断位并返回，不做落盘保证。
            Thread.currentThread().interrupt()
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
     * 清空日志：入队 ClearBarrier，writer 处理到此命令时先写完前序 Append，
     * 再由 writer 线程删除日志目录下所有文件，最后 countDown 唤醒本等待。
     *
     * 文件删除在 writer 线程执行（不在调用线程），保证不会与 writer 的 writeBatch
     * 并发产生「文件已删、writer 随后 FileWriter(append) 重建并写回旧日志」的复活竞态：
     * ClearBarrier 在队列中按序处理，其后的 Append 一定在删除之后才写盘。
     * 最多等待 [FLUSH_TIMEOUT_MS]：writer 若因不可控 Error 死亡，清空不能永久挂起。
     * 未初始化时直接返回。
     */
    fun clearLogs() {
        val latch = CountDownLatch(1)
        synchronized(lock) {
            if (!initialized) return
            queue.addLast(LogWriterCommand.ClearBarrier(latch))
            lock.notifyAll()
        }
        try {
            latch.await(FLUSH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (_: InterruptedException) {
            // 调用方线程被中断：恢复中断位并返回。writer 仍会按序处理 ClearBarrier 删除文件。
            Thread.currentThread().interrupt()
        }
    }

    /**
     * writer 线程主循环：wait → drain 整个命令队列 → 按序处理命令 → 写盘/删文件。
     */
    private fun writerLoop() {
        while (true) {
            val commands: List<LogWriterCommand>
            synchronized(lock) {
                while (queue.isEmpty()) {
                    try {
                        lock.wait()
                    } catch (_: InterruptedException) {
                        // 常驻 daemon 线程：中断只当作虚假唤醒，不恢复中断位，线程绝不退出。
                    }
                }
                // drain 整个队列到本地列表，队列清空供后续 enqueue 使用。
                commands = queue.toList()
                queue.clear()
            }
            processCommands(commands)
        }
    }

    /**
     * 在 lock 外按入队顺序处理命令。连续 Append 收集为 batch 写盘；
     * 遇到 FlushBarrier/ClearBarrier 先写完当前 batch 再执行屏障语义。
     */
    private fun processCommands(commands: List<LogWriterCommand>) {
        val batch = ArrayList<LogRequest>()
        for (cmd in commands) {
            when (cmd) {
                is LogWriterCommand.Append -> {
                    batch.add(cmd.request)
                }
                is LogWriterCommand.FlushBarrier -> {
                    if (batch.isNotEmpty()) {
                        writeBatch(batch)
                        batch.clear()
                    }
                    cmd.latch.countDown()
                }
                is LogWriterCommand.ClearBarrier -> {
                    if (batch.isNotEmpty()) {
                        writeBatch(batch)
                        batch.clear()
                    }
                    // 文件删除在 writer 线程执行，与 writeBatch 串行，无并发竞态。
                    deleteLogFiles()
                    cmd.latch.countDown()
                }
            }
        }
        // 尾部连续 Append 写盘。
        if (batch.isNotEmpty()) {
            writeBatch(batch)
        }
    }

    /** 删除日志目录下所有文件（仅由 writer 线程调用）。 */
    private fun deleteLogFiles() {
        val logDir = AndroidDataRoot.logsDir()
        if (logDir.exists()) {
            logDir.listFiles()?.forEach { it.delete() }
        }
    }

    /** 把一整批请求 append 到当前日志文件，每个 batch 写完即 flush。 */
    private fun writeBatch(batch: List<LogRequest>) {
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
