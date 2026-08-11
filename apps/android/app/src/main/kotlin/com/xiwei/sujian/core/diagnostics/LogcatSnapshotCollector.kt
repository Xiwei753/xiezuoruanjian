package com.xiwei.sujian.core.diagnostics

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.util.concurrent.TimeUnit

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

    /** logcat -d 正常应立刻退出；等不到就强杀，导出不能被子进程挂住。 */
    private const val WAIT_FOR_EXIT_SECONDS = 5L

    /**
     * 执行 logcat -d 抓取快照，脱敏后写入 [destDir]/logcat.txt。
     * 失败时写占位文件，不抛异常。
     */
    fun collect(destDir: File) {
        try {
            val process =
                ProcessBuilder("logcat", "-d", "-v", "threadtime")
                    .redirectErrorStream(true)
                    .start()
            val (bytes, capped) = readBounded(process.inputStream)
            if (capped) {
                // 超过体积上限：停止读取并终止 logcat，避免继续读入数十 MB 撑爆导出内存。
                process.destroy()
            }
            process.inputStream.close()
            if (!process.waitFor(WAIT_FOR_EXIT_SECONDS, TimeUnit.SECONDS)) {
                process.destroyForcibly()
            }
            val trimmed = truncateBytes(bytes, MAX_SNAPSHOT_BYTES)
            val redacted = DiagnosticsLogger.redact(String(trimmed, Charsets.UTF_8))
            File(destDir, OUTPUT_NAME).writeText(redacted)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            File(destDir, OUTPUT_NAME).writeText("logcat capture failed: $safeMsg")
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
