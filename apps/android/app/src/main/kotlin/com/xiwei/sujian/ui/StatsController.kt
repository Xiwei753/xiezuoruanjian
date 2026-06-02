package com.xiwei.sujian.ui

import android.widget.FrameLayout
import android.widget.TextView
import android.view.LayoutInflater
import android.view.View
import com.xiwei.sujian.R
import com.xiwei.sujian.data.StatsBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.model.WritingWritingStatsSummary
import com.xiwei.sujian.model.WritingStatsSummary
import com.xiwei.sujian.model.ProjectWritingStatsSummary
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import org.json.JSONObject

/**
 * StatsController — 写作统计控制器
 *
 * 加载并展示今日、本周、本月的写作统计数据。
 *
 * ## 架构定位
 * - MainActivity → StatsController → StatsBridge → AppServiceBridge → UniFFI → Rust Core
 *
 * ## 职责边界
 * - **做**：加载统计数据、格式化展示、计算字数/时长/速度
 * - **不做**：统计数据的存储（由 Rust Core 负责）
 *
 * ## 使用场景
 * - MainActivity 统计标签页的数据展示
 * - 展示今日/本周/本月的写作量和速度
 */
class StatsController(
    private val activity: MainActivity,
    private val bridge: StatsBridge,
    private val tabContainer: FrameLayout
) {

    private lateinit var tvTodayStats: TextView
    private lateinit var tvWeekStats: TextView
    private lateinit var tvProjectStats: TextView
    private var isInitialized = false

    fun initialize() {
        if (!isInitialized) {
            val view = LayoutInflater.from(activity).inflate(R.layout.layout_stats, tabContainer, false)
            tabContainer.removeAllViews()
            tabContainer.addView(view)

            tvTodayStats = view.findViewById(R.id.tvTodayStats)
            tvWeekStats = view.findViewById(R.id.tvWeekStats)
            tvProjectStats = view.findViewById(R.id.tvProjectStats)

            isInitialized = true
        }
        loadStats()
    }

    private fun loadStats() {
        val dateFormat = SimpleDateFormat("yyyy-MM-dd", Locale.getDefault())
        val today = Date()
        val todayStr = dateFormat.format(today)

        val cal = Calendar.getInstance()
        cal.time = today
        cal.set(Calendar.DAY_OF_WEEK, cal.firstDayOfWeek)
        val weekStartStr = dateFormat.format(cal.time)

        CoroutineScope(Dispatchers.IO).launch {
            val todayStatsResult = bridge.getWritingWritingStatsSummary(todayStr, todayStr)
            val weekStatsResult = bridge.getWritingWritingStatsSummary(weekStartStr, todayStr)
            val projectStatsResult = bridge.getWritingStatsByProject(weekStartStr, todayStr)

            withContext(Dispatchers.Main) {
                tvTodayStats.text = formatSummary(todayStatsResult)
                tvWeekStats.text = formatSummary(weekStatsResult)
                tvProjectStats.text = formatProjectStats(projectStatsResult)
            }
        }
    }

    private fun formatSummary(result: BridgeResult<WritingStatsSummary>): String {
        return when (result) {
            is BridgeResult.Success -> {
                val stats = result.data
                val chars = stats.totalHumanTypedChars ?: 0
                val activeSeconds = stats.totalActiveSeconds ?: 0
                val duration = if (activeSeconds > 3600) {
                    "${activeSeconds / 3600}时${(activeSeconds % 3600) / 60}分"
                } else {
                    "${activeSeconds / 60}分"
                }
                "纯输入: $chars 字\n活跃时长: $duration"
            }
            is BridgeResult.Error -> "加载失败: ${result.message}"
            BridgeResult.NotLoaded -> "未加载"
        }
    }

    private fun formatProjectStats(result: BridgeResult<ProjectWritingStatsSummary>): String {
        return when (result) {
            is BridgeResult.Success -> {
                val statsMap = result.data.projects
                if (statsMap.isNullOrEmpty()) {
                    return "无项目数据"
                }
                val sb = StringBuilder()
                for (projectStats in statsMap) {
                    val chars = projectStats.humanTypedChars ?: 0
                    val title = projectStats.projectTitle ?: "未命名"
                    sb.append("项目: $title\n")
                    sb.append("输入: $chars 字\n\n")
                }
                sb.toString()
            }
            is BridgeResult.Error -> "加载失败: ${result.message}"
            BridgeResult.NotLoaded -> "未加载"
        }
    }
}
