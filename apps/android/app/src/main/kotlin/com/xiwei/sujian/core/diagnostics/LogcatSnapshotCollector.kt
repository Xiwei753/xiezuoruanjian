package com.xiwei.sujian.core.diagnostics

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/**
 * Logcat 快照采集器（Issue #612 三、3.1）。
 *
 * 在导出诊断包时调用 [collect]，执行 `logcat -d -v threadtime` 获取当前进程可见的
 * Logcat 快照（dump 一次后退出），体积上限 2 MiB，脱敏后写入 [destDir]/logcat.txt。
 *
 * - 只在导出时按需调用，不常驻后台抓取。
 * - 读取即受限：[readBounded] 最多读入 2 MiB + 一个 chunk 的余量就停止并销毁
 *   logcat 子进程，不会把整台设备 logcat 缓冲区（可能数十 MB）全部读进内存
 *   再截断（Issue #612 三、3.1 “限制最大体积”是对读取本身的约束）。
 * - 单一 deadline 管读取与等待退出（Issue #612 评论 5.2）：一个 deadline
 *   （[WAIT_FOR_EXIT_SECONDS]）同时约束 stdout 读取（future.get）与进程退出
 *   （waitFor），不会出现“读取 5s + 等待 5s = 10s”。任何退出路径（成功/超时/异常）
 *   都经 finally 回收：destroyForcibly 关闭管道使 reader 的 InputStream.read() 抛
 *   IOException 解除阻塞，shutdownNow 中断 reader 线程，二者配合保证 reader task
 *   与 logcat 子进程不泄漏。
 * - 截断按 UTF-8 字符边界进行，不会产生半个字符。
 * - 通过 [DiagnosticsLogger.redact] 脱敏后再落盘。
 * - 失败时写一个包含错误信息的占位文件，不抛异常。
 */
internal object LogcatSnapshotCollector {
    /** 快照体积上限：2 MiB。internal 供单测直接引用。 */
    internal const val MAX_SNAPSHOT_BYTES = 2 * 1024 * 1024L

    /** 读取步长：chunk 读取的余量，保证截断处有足够字节做 UTF-8 边界回退。 */
    internal const val READ_CHUNK_BYTES = 16 * 1024

    private const val OUTPUT_NAME = "logcat.txt"

    /**
     * logcat -d 正常应立刻退出；超时作用在整个采集过程（读取 + 等待退出），
     * 等不到就强杀，导出不能被子进程挂住（Issue #612 评论 3.2）。
     */
    private const val WAIT_FOR_EXIT_SECONDS = 5L

    /**
     * 执行 logcat -d 抓取快照，脱敏后写入 [destDir]/logcat.txt。
     * 失败时写占位文件，不抛异常。
     *
     * 单一 deadline 管读取与等待退出（Issue #612 评论 5.2）：一个 deadline
     * （[WAIT_FOR_EXIT_SECONDS]）同时约束 stdout 读取（future.get）与进程退出
     * （waitFor），不会出现“读取 5s + 等待 5s = 10s”。任何退出路径（成功/超时/异常）
     * 都经 finally 回收子进程与 reader executor，不泄漏。
     */
    fun collect(destDir: File) {
        collectCommand(destDir, listOf("logcat", "-d", "-v", "threadtime"))
    }

    /**
     * 单一 deadline 管读取与等待退出，任何退出路径都回收子进程与 reader executor
     * （Issue #612 评论 5.2）。提取为 internal 供单测注入可控行令验证超时与清理，
     * 不依赖真实 logcat 进程。
     *
     * - 一个 deadline（[WAIT_FOR_EXIT_SECONDS]）同时约束 stdout 读取（future.get）与
     *   进程退出（waitFor），不会出现“读取 5s + 等待 5s = 10s”。
     * - 任何退出路径（成功/超时/异常）都经 finally 回收：destroyForcibly 关闭管道使
     *   reader 的 InputStream.read() 抛 IOException 解除阻塞，shutdownNow 中断 reader 线程，
     *   二者配合保证 reader task 与 logcat 子进程不泄漏。
     * - 失败时写占位文件，不抛异常。
     */
    internal fun collectCommand(
        destDir: File,
        command: List<String>,
    ) {
        var process: Process? = null
        var future: Future<Pair<ByteArray, Boolean>>? = null
        val executor = Executors.newSingleThreadExecutor()
        try {
            val deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_FOR_EXIT_SECONDS)
            process =
                ProcessBuilder(command)
                    .redirectErrorStream(true)
                    .start()
            future =
                executor.submit<Pair<ByteArray, Boolean>> {
                    process!!.inputStream.use { readBounded(it) }
                }

            val remainingForRead = deadlineNanos - System.nanoTime()
            if (remainingForRead <= 0) throw TimeoutException("logcat capture timed out")
            val (bytes, capped) = future.get(remainingForRead, TimeUnit.NANOSECONDS)

            if (capped && process.isAlive) process.destroy()

            val remainingForExit = deadlineNanos - System.nanoTime()
            if (remainingForExit <= 0 || !process.waitFor(remainingForExit, TimeUnit.NANOSECONDS)) {
                throw TimeoutException("logcat capture timed out")
            }

            val trimmed = truncateBytes(bytes, MAX_SNAPSHOT_BYTES)
            File(destDir, OUTPUT_NAME).writeText(
                DiagnosticsLogger.redact(String(trimmed, Charsets.UTF_8)),
            )
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            File(destDir, OUTPUT_NAME).writeText("logcat capture failed: $safeMsg")
        } finally {
            process?.let { if (it.isAlive) it.destroyForcibly() }
            future?.cancel(true)
            executor.shutdownNow()
        }
    }

    /**
     * 以 [chunkSize] 为步长读取 [stream]，读满 [maxBytes] + 一个 chunk 的余量即停止。
     *
     * @return 已读字节与是否触顶（capped=true 时调用方应销毁子进程，剩余数据不再读）。
     * 提取为 internal 纯函数便于单测验证体积上限与触顶行为，不需要真实 logcat 进程。
     */
    internal fun readBounded(
        stream: InputStream,
        maxBytes: Long = MAX_SNAPSHOT_BYTES,
        chunkSize: Int = READ_CHUNK_BYTES,
    ): Pair<ByteArray, Boolean> {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(chunkSize)
        val hardCap = maxBytes + chunkSize
        while (true) {
            val n = stream.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            if (out.size() >= hardCap) {
                return out.toByteArray() to true
            }
        }
        return out.toByteArray() to false
    }

    /**
     * 在 [maxBytes] 字节处往前找 UTF-8 字符边界（非续字节）截断。
     * 续字节匹配 10xxxxxx 即 (byte and 0xC0) == 0x80；UTF-8 字符最多 4 字节，
     * 最多回退 3 次。返回的字节数组解码后不会出现半个字符。
     */
    internal fun truncateBytes(
        bytes: ByteArray,
        maxBytes: Long = MAX_SNAPSHOT_BYTES,
    ): ByteArray {
        if (bytes.size <= maxBytes) return bytes
        var end = maxBytes.toInt()
        while (end > 0 && (bytes[end].toInt() and 0xC0) == 0x80) {
            end--
        }
        return bytes.copyOf(end)
    }
}
