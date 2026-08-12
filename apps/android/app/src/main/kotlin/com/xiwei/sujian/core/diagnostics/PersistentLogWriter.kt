package com.xiwei.sujian.core.diagnostics

import android.content.Context
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

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

    /**
     * flush 屏障：writer 处理到此命令时先写完前序 Append，再 countDown 唤醒等待者。
     *
     * [persisted] 是 writer 线程 → 调用方线程的结果位（Issue #612 评论 3.1）：
     * writer 处理屏障时把当前 [PersistentLogWriter.persistenceHealthy] 写入。
     * 调用方在 latch countDown 后读取，据此判断“前序日志确实落盘”还是
     * “屏障完成但写盘失败/此前已失败”——不得把缺日志的导出伪装成完整导出。
     * 失败状态不在普通 flush 后清掉（前面的日志已经丢了，后续导出不能再声称完整）；
     * 只有成功的 ClearBarrier 明确把旧日志清空后才会重置为 true。
     */
    data class FlushBarrier(
        val latch: CountDownLatch,
        val persisted: AtomicBoolean,
    ) : LogWriterCommand

    /**
     * clear 屏障：writer 处理到此命令时先写完前序 Append，再删除日志目录所有文件，
     * 最后 countDown 唤醒等待者。删除结果写入 [deleted]（writer 线程 → 调用方线程），
     * 调用方据此区分“屏障完成但删除失败”（不得伪装成清空成功）。
     */
    data class ClearBarrier(
        val latch: CountDownLatch,
        val deleted: AtomicBoolean,
    ) : LogWriterCommand
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
 * 失败语义（Issue #612 评论 3）：writerLoop 不整批吞异常。命令逐条处理，
 * FlushBarrier/ClearBarrier 的 latch 在 try/finally 中必定 countDown——单条命令抛
 * 出 VM 级 Error（writeBatch/deleteLogFiles 只捕获可恢复的 Exception）会终止 writer
 * 线程，但同批后续 barrier 的等待者不会因 latch 丢失而永久挂起，只能等超时拿到
 * false。[flushBlocking]/[clearLogs] 返回 Boolean：latch 在超时前完成返回 true，
 * 超时/中断返回 false，调用方不得把失败伪装成成功。
 *
 * 落盘健康位（Issue #612 评论 3.1）：[persistenceHealthy] 只由 writer 线程读写，
 * 记录“自上次成功 ClearBarrier 起所有写盘是否全部成功”。写盘失败后置 false，
 * FlushBarrier 把它写入 [LogWriterCommand.FlushBarrier.persisted] 传回调用方——
 * flushBlocking 返回 completed && persisted.get()，导出据此停止“缺日志仍打包完整 zip”。
 * 失败状态不在普通 flush 后清掉（前面的日志已经丢了）；只有成功的 ClearBarrier
 * 明确把旧日志清空后才重置为 true。
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
     * #623 评论 3：当前构建身份。init 时设置，决定日志文件名 build key。
     * 由 init 调用线程写入，由 writer 线程读取；用 @Volatile 保证可见性。
     */
    @Volatile private var buildIdentity: DiagnosticsBuildIdentity? = null

    /**
     * writer 线程私有落盘健康位（Issue #612 评论 3.1）。只在 writer 线程访问，
     * 无需 volatile/atomic。写盘失败后置 false，FlushBarrier 据此告知调用方
     * “前序日志未完整落盘”；只有成功的 ClearBarrier 重置为 true。
     */
    private var persistenceHealthy = true

    /**
     * 初始化并启动 writer 线程。幂等：重复调用无副作用。
     * [context] 保留供未来绑定应用生命周期；当前日志目录由 [AndroidDataRoot] 决定。
     */
    fun init(
        context: Context,
        identity: DiagnosticsBuildIdentity,
    ) {
        synchronized(lock) {
            if (initialized) return
            initialized = true
            enabled = true
            buildIdentity = identity
            val thread = Thread({ writerLoop() }, "sujian-logger")
            thread.priority = Thread.MIN_PRIORITY
            thread.isDaemon = true
            thread.start()
        }
        // 目录创建推迟到 writer 线程第一次 writeBatch()（Issue #612 评论 3.5）：
        // init() 的调用线程不做任何文件 I/O，日志目录只由 sujian-logger 线程创建。
    }

    /**
     * 向后兼容重载：未提供构建身份时从 BuildConfig 生成。
     *
     * 生产路径走 [DiagnosticsLogger.init] -> init(context, identity)；
     * 此重载供测试直接初始化 [PersistentLogWriter] 时使用，确保日志文件名同样
     * 按当前 BuildConfig 的 build key 分界（#623 评论 3）。
     */
    fun init(context: Context) {
        init(context, DiagnosticsBuildIdentity.fromBuildConfig())
    }

    /**
     * #623 评论 3：当前构建的日志文件名。带 build key 时为
     * sujian-current-v1234-e2ce827-ai-debug.log；未设置身份时回退到裸
     * sujian-current.log（仅用于旧历史文件兼容，不作为新构建的当前文件）。
     */
    private fun currentLogFileName(): String {
        val identity = buildIdentity
        return if (identity != null) {
            "$LOG_PREFIX-${identity.buildKey}.log"
        } else {
            "$LOG_PREFIX.log"
        }
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
     *
     * 落盘结果位（Issue #612 评论 3.1）：writer 处理屏障时把 [persistenceHealthy]
     * 写入 [LogWriterCommand.FlushBarrier.persisted]。只有 latch 在超时前完成
     * **且** persisted 为 true 才返回 true——写盘失败/此前已失败时返回 false，
     * 导出据此不得把缺日志的 zip 伪装成完整导出。
     *
     * @return latch 在超时前完成且前序日志确实落盘返回 true；超时、调用线程被中断
     * （恢复中断位）或写盘失败返回 false。未初始化时没有可 flush 的日志，返回 true。
     */
    fun flushBlocking(): Boolean {
        val latch = CountDownLatch(1)
        val persisted = AtomicBoolean(false)
        synchronized(lock) {
            if (!initialized) return true
            queue.addLast(LogWriterCommand.FlushBarrier(latch, persisted))
            lock.notifyAll()
        }
        val completed =
            try {
                latch.await(FLUSH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                // 调用方线程被中断：恢复中断位并返回 false，不做落盘保证。
                Thread.currentThread().interrupt()
                false
            }
        return completed && persisted.get()
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
     *
     * @return 只有“latch 在超时前完成”且“文件确实全部删除成功”才返回 true；
     * 超时、调用线程被中断（恢复中断位，writer 仍会按序处理 ClearBarrier 删除文件）、
     * 或删除失败（目录状态异常/文件删除失败）都返回 false——调用方不得把删除失败
     * 伪装成清空成功（Issue #612 评论 3.4）。未初始化时没有可清的日志，返回 true。
     */
    fun clearLogs(): Boolean {
        val latch = CountDownLatch(1)
        val deleted = AtomicBoolean(false)
        synchronized(lock) {
            if (!initialized) return true
            queue.addLast(LogWriterCommand.ClearBarrier(latch, deleted))
            lock.notifyAll()
        }
        val completed =
            try {
                latch.await(FLUSH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                // 调用方线程被中断：恢复中断位并返回 false。
                Thread.currentThread().interrupt()
                false
            }
        return completed && deleted.get()
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
            // 不整批吞异常（Issue #612 评论 3.1）：命令逐条处理，barrier 的 latch
            // 在 try/finally 中必定释放；writeBatch/deleteLogFiles 只捕获可恢复的
            // Exception，VM 级 Error 会终止 writer 线程——调用方通过 flushBlocking/
            // clearLogs 的 Boolean 返回值感知失败，而不是被 latch 永久挂起或假装成功。
            processCommands(commands)
        }
    }

    /**
     * 在 lock 外按入队顺序处理命令。连续 Append 仍收集为 batch 写盘（不改命令
     * 队列顺序）；FlushBarrier/ClearBarrier 的 latch 用 try/finally 保证必定释放：
     * 即使 flushBatch/deleteLogFiles 抛出 VM 级 Error，等待者也不会丢失 latch。
     */
    private fun processCommands(commands: List<LogWriterCommand>) {
        val batch = ArrayList<LogRequest>()
        for (cmd in commands) {
            when (cmd) {
                is LogWriterCommand.Append -> batch.add(cmd.request)
                is LogWriterCommand.FlushBarrier -> handleFlushBarrier(cmd, batch)
                is LogWriterCommand.ClearBarrier -> handleClearBarrier(cmd, batch)
            }
        }
        // 尾部连续 Append 写盘：尾部 batch 写失败要更新 persistenceHealthy。
        val tailOk = flushBatch(batch)
        if (!tailOk) {
            persistenceHealthy = false
        }
    }

    /** 处理 FlushBarrier：写完前序 batch，把 persistenceHealthy 写入结果位（Issue #612 评论 3.1）。 */
    private fun handleFlushBarrier(
        cmd: LogWriterCommand.FlushBarrier,
        batch: MutableList<LogRequest>,
    ) {
        try {
            val batchOk = flushBatch(batch)
            persistenceHealthy = persistenceHealthy && batchOk
            cmd.persisted.set(persistenceHealthy)
        } finally {
            cmd.latch.countDown()
        }
    }

    /**
     * 处理 ClearBarrier：写完前序 batch，再删除日志文件（Issue #612 评论 3.1）。
     * 只有删除成功才把 persistenceHealthy 重置为 true。
     */
    private fun handleClearBarrier(
        cmd: LogWriterCommand.ClearBarrier,
        batch: MutableList<LogRequest>,
    ) {
        try {
            flushBatch(batch)
            // 文件删除在 writer 线程执行，与 writeBatch 串行，无并发竞态；
            // 删除结果经 cmd.deleted 传回调用方（不得把失败伪装成成功）。
            val deleted = deleteLogFiles(AndroidDataRoot.logsDir())
            // 只有删除成功才把 persistenceHealthy 重置为 true（Issue #612 评论 3.1）：
            // 旧日志已清空，后续可重新声称完整。删除失败时保留原健康位，
            // 调用方通过 deleted 感知清空失败。
            if (deleted) {
                persistenceHealthy = true
            }
            cmd.deleted.set(deleted)
        } finally {
            cmd.latch.countDown()
        }
    }

    /**
     * 把当前累积的 Append batch 写盘并清空（仅由 writer 线程调用）。
     *
     * @return 写盘成功返回 true；空 batch 返回 true；写盘失败返回 false
     * （调用方据此更新 [persistenceHealthy]）。
     */
    private fun flushBatch(batch: MutableList<LogRequest>): Boolean {
        if (batch.isEmpty()) return true
        val ok = writeBatch(batch)
        batch.clear()
        return ok
    }

    /**
     * 删除日志目录下的所有文件（仅由 writer 线程调用）。
     *
     * - 只删除文件；子目录及其内容属于未知数据，绝不触碰（仓库安全边界）。
     * - 目录不存在视为无可删内容，返回 true；路径存在但不是目录、listFiles 失败、
     *   任一文件删除失败都返回 false——ClearBarrier 把结果传回 clearLogs，
     *   调用方据此显示“清空失败”而不是假装已经清空（Issue #612 评论 3.4）。
     * - 删除失败是可恢复 I/O 故障：latch 由 ClearBarrier 的 finally 释放，
     *   writer 线程继续存活，不因一次删除失败而死亡（Issue #612 评论 3）。
     */
    internal fun deleteLogFiles(logDir: File): Boolean {
        return try {
            if (!logDir.exists()) return true
            if (!logDir.isDirectory) return false
            val files = logDir.listFiles() ?: return false
            var allDeleted = true
            for (file in files) {
                if (file.isFile && !file.delete()) {
                    allDeleted = false
                }
            }
            allDeleted
        } catch (_: Exception) {
            false
        }
    }

    /**
     * 把一整批请求 append 到当前日志文件，每个 batch 写完即 flush。
     *
     * 改用 `FileOutputStream.bufferedWriter(UTF_8)` + `write`/`newLine` 落盘
     * （Issue #612 评论 5.1）：旧实现用 `PrintWriter(FileWriter(...))`，PrintWriter
     * 的写入方法会吞掉底层 IOException（只设内部 error flag，不抛），磁盘在打开文件
     * 后写失败/flush 失败时外层 catch 根本收不到——writeBatch 返回 true，
     * [persistenceHealthy] 被绕过，[flushBlocking] 返回 true 但日志没落盘，导出会
     * 打包一个缺日志的 zip。BufferedWriter 的 write/newLine/flush 真实抛 IOException，
     * catch 精确到 [IOException] 与 [SecurityException]（可恢复的 I/O / 权限故障）；
     * 其它 RuntimeException（编程错误）向上传播终止 writer 线程，调用方经 Boolean
     * 返回值感知失败。
     *
     * @return 写盘成功返回 true；写盘失败返回 false（Issue #612 评论 3.1）。
     * I/O 失败：丢弃本批次，writer 线程继续存活处理后续日志。
     */
    private fun writeBatch(batch: List<LogRequest>): Boolean =
        try {
            ensureLogsDirOrThrow()
            val currentFile = File(AndroidDataRoot.logsDir(), currentLogFileName())
            rotateIfNeeded(currentFile)
            FileOutputStream(currentFile, true)
                .bufferedWriter(Charsets.UTF_8)
                .use { writer ->
                    for (req in batch) {
                        val ts = timestampFormat.format(Date(req.timestampMs))
                        writer.write("$ts ${req.level}/${req.tag}: ${req.message}")
                        writer.newLine()
                    }
                    writer.flush()
                }
            true
        } catch (_: IOException) {
            false
        } catch (_: SecurityException) {
            false
        }

    /** 当前文件超过 1 MiB 时重命名为带时间戳的轮转文件，并裁剪到 5 个。 */
    private fun rotateIfNeeded(file: File) {
        if (!file.exists() || file.length() < MAX_FILE_SIZE) return
        val logDir = file.parentFile ?: return
        // #623 评论 3：轮转文件名带 build key，围绕当前构建身份轮转，
        // 不丢掉构建身份。baseName 例如 sujian-current-v1234-e2ce827-ai-debug。
        val baseName = file.nameWithoutExtension
        val rotated = File(logDir, "$baseName-${System.currentTimeMillis()}.log")
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

    /**
     * 确保日志目录存在（仅由 writer 线程调用）。不内部吞异常（Issue #612 评论 3.1）：
     * mkdirs() 失败会抛 SecurityException/IOException，由 [writeBatch] 的
     * catch(Exception) 统一捕获返回 false——调用方据此感知写盘失败。
     */
    private fun ensureLogsDirOrThrow() {
        AndroidDataRoot.logsDir().mkdirs()
    }
}
