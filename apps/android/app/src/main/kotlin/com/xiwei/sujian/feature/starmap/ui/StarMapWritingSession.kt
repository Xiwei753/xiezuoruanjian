package com.xiwei.sujian.feature.starmap.ui

import android.os.SystemClock
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.delay
import uniffi.writer_core.WritingActivitySegmentInputDto
import java.util.UUID

/**
 * Issue #1036：星图前台写作计时。
 *
 * 第一次真实人工操作（按下画布）才开始计时；思考与停留继续累计；离开星图
 * 或切后台时结算。纯 MCP/后台写入不会经过这里，不能冒充人工创作时间。
 * 星图 ID 只放进 `starmapId`，chapterId 保持为空。
 */
internal class StarMapWritingSession(
    private val starMapId: String,
    private val projectId: String?,
    private val repository: WritingStatsRepository,
) {
    private var active = false
    private var sessionId = ""
    private var startAtWallMs = 0L
    private var startAtMonoMs = 0L

    fun start() {
        if (active) return
        active = true
        sessionId = UUID.randomUUID().toString()
        startAtMonoMs = SystemClock.elapsedRealtime()
        startAtWallMs = System.currentTimeMillis()
    }

    /** 结算当前切片；心跳时继续累计，后台/离开时由调用方停止。 */
    fun flush(reason: String) {
        if (!active) return
        val endMono = SystemClock.elapsedRealtime()
        val endWall = System.currentTimeMillis()
        if (endMono > startAtMonoMs && sessionId.isNotEmpty()) {
            repository.recordWritingActivitySegment(
                WritingActivitySegmentInputDto(
                    platform = "android",
                    projectId = projectId.orEmpty(),
                    volumeId = "",
                    chapterId = "",
                    starmapId = starMapId,
                    sessionId = sessionId,
                    startAtMs = startAtWallMs,
                    endAtMs = endWall,
                    pauseReason = reason,
                    eventId = UUID.randomUUID().toString(),
                ),
            )
        }
        startAtMonoMs = endMono
        startAtWallMs = endWall
    }

    fun heartbeat() = flush("heartbeat")

    /** 退到后台：立即结算并停止计时；回前台等下一次真实操作再开新切片。 */
    fun pause(reason: String) {
        flush(reason)
        active = false
    }

    fun close() {
        flush("leftEditor")
        active = false
    }
}

@Composable
internal fun rememberStarMapWritingSession(
    starMapId: String,
    projectId: String?,
): StarMapWritingSession {
    val repository = LocalSujianAppDependencies.current.statsRepository
    val lifecycleOwner = LocalLifecycleOwner.current
    val session = remember(starMapId, projectId) {
        StarMapWritingSession(starMapId, projectId, repository)
    }
    LaunchedEffect(session) {
        while (true) {
            delay(30_000)
            session.heartbeat()
        }
    }
    DisposableEffect(session, lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) {
                session.pause("background")
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            session.close()
        }
    }
    return session
}
