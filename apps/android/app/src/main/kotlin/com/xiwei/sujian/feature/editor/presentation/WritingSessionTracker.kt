package com.xiwei.sujian.feature.editor.presentation

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import java.util.UUID
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.WritingActivitySegmentInputDto

/**
 * Issue #981：真实前台写作会话计时。
 *
 * 平台只负责两类事实：正文页/窗口是否仍在前台可写，以及哪次编辑事务是真人输入。
 * 计时状态与 Core 的 WritingSessionClock 对齐：
 * - 第一次真人编辑才开始计时（粘贴/AI/撤销重做不算，删除算真人编辑）；
 * - 写作期间无输入也继续计时，思考不会被「5 分钟无输入」扣掉；
 * - 退后台、失去前台、离开正文页立即结算当前片段；
 * - 回到前台不补算离开期间，下一次真人编辑才开启新片段。
 *
 * 这里不计算字/时；只把已测片段交给 Core 的串行统计 writer。
 */
internal class WritingSessionTracker(
    private val recordSegment: (WritingActivitySegmentInputDto) -> Unit,
) {
    private enum class State { Idle, Writing, Paused }

    private var state = State.Idle
    private var sessionId = ""
    private var projectId = ""
    private var volumeId = ""
    private var chapterId = ""
    private var monotonicAnchorMs = 0L
    private var wallAnchorMs = 0L
    private var sliceStartMonoMs = 0L
    private var foreground = false
    private var sessionTypedChars = 0L
    private var completedActiveMs = 0L
    private val handler = Handler(Looper.getMainLooper())

    private val heartbeat = object : Runnable {
        override fun run() {
            if (state != State.Writing) return
            val now = SystemClock.elapsedRealtime()
            if (now - sliceStartMonoMs >= FLUSH_INTERVAL_MS) {
                flush(now, "heartbeat")
                sliceStartMonoMs = now
            }
            handler.postDelayed(this, FLUSH_INTERVAL_MS)
        }
    }

    /** 真人编辑事务入口。章节切换先结算旧章节，再从当前时刻开新片段。 */
    fun onHumanEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
        insertedChars: Int,
        occurredAtMs: Long,
    ) {
        // 后台/离开正文页后的迟到 IME 提交不能再开启计时片段。
        if (!foreground || chapterId.isEmpty()) return
        val now = SystemClock.elapsedRealtime()
        sessionTypedChars += insertedChars.coerceAtLeast(0)
        if (state == State.Writing) {
            if (this.projectId != projectId || this.volumeId != volumeId || this.chapterId != chapterId) {
                flush(now, "chapterSwitch")
                this.projectId = projectId
                this.volumeId = volumeId
                this.chapterId = chapterId
                sliceStartMonoMs = now
            }
            return
        }

        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        if (state == State.Idle) {
            sessionId = UUID.randomUUID().toString()
            // 与本次编辑事件共用同一墙钟锚点，确保首笔字数落在计时区间内。
            wallAnchorMs = occurredAtMs
            monotonicAnchorMs = now
            sessionTypedChars = insertedChars.coerceAtLeast(0).toLong()
            completedActiveMs = 0L
        }
        // 从暂停恢复：不把离开期间补进来，从这次编辑重新起算。
        sliceStartMonoMs = now
        state = State.Writing
        handler.removeCallbacks(heartbeat)
        handler.postDelayed(heartbeat, FLUSH_INTERVAL_MS)
    }

    /** 重新回到前台只解除暂停状态，不补算离开期间；等下一次真人编辑。 */
    fun resume() {
        foreground = true
    }

    /** 当前会话累计的真人输入字数。 */
    fun sessionTypedChars(): Long = sessionTypedChars

    /** 当前会话累计的真实计时秒数（已完成切片 + 正在进行的切片）。 */
    fun sessionActiveSeconds(): Long {
        val pending =
            if (state == State.Writing) {
                (SystemClock.elapsedRealtime() - sliceStartMonoMs).coerceAtLeast(0L)
            } else {
                0L
            }
        return (completedActiveMs + pending) / 1000L
    }

    /**
     * 当前会话真实累计字数 / 真实计时秒数的字/时。
     * 样本不足 30 秒时返回 0，避免刚输入几个字就显示几千字/时。
     */
    fun currentCharsPerHour(): Long {
        if (sessionTypedChars <= 0L) return 0L
        val seconds = sessionActiveSeconds()
        if (seconds < MIN_SPEED_SAMPLE_SECONDS) return 0L
        return sessionTypedChars * 3600L / seconds
    }

    /** Activity/正文页离开前台：立即结算并暂停。重复调用不重复计时。 */
    fun pause(reason: String) {
        foreground = false
        if (state != State.Writing) return
        flush(SystemClock.elapsedRealtime(), reason)
        state = State.Paused
        handler.removeCallbacks(heartbeat)
    }

    /** 关闭正文/退出页面：结算最后不足 30 秒的片段并结束会话。 */
    fun close(reason: String) {
        foreground = false
        if (state == State.Writing) {
            flush(SystemClock.elapsedRealtime(), reason)
        }
        state = State.Idle
        sessionId = ""
        projectId = ""
        volumeId = ""
        chapterId = ""
        handler.removeCallbacks(heartbeat)
    }

    private fun flush(monoMs: Long, reason: String) {
        val durationMs = monoMs - sliceStartMonoMs
        if (durationMs <= 0L || sessionId.isEmpty() || chapterId.isEmpty()) return
        completedActiveMs += durationMs
        val startAtMs = wallAnchorMs + (sliceStartMonoMs - monotonicAnchorMs)
        val endAtMs = wallAnchorMs + (monoMs - monotonicAnchorMs)
        recordSegment(
            WritingActivitySegmentInputDto(
                platform = "android",
                projectId = projectId,
                volumeId = volumeId,
                chapterId = chapterId,
                sessionId = sessionId,
                startAtMs = startAtMs,
                endAtMs = endAtMs,
                pauseReason = reason,
                eventId = UUID.randomUUID().toString(),
            ),
        )
    }

    companion object {
        private const val FLUSH_INTERVAL_MS = 30_000L
        private const val MIN_SPEED_SAMPLE_SECONDS = 30L
    }
}

/** 只有真人输入/删除才建立或延续写作会话；粘贴、AI、撤销重做等不算。 */
internal fun EditorTransactionCauseDto.isHumanWritingActivity(): Boolean =
    this == EditorTransactionCauseDto.TYPING ||
        this == EditorTransactionCauseDto.TYPING_COMMIT ||
        this == EditorTransactionCauseDto.IME_COMPOSITION ||
        this == EditorTransactionCauseDto.DELETE
