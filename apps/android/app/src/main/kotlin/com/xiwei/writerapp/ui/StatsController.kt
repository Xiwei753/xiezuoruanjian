package com.xiwei.writerapp.ui

import android.widget.FrameLayout
import android.widget.TextView
import android.view.LayoutInflater
import android.view.View
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.NativeCoreBridge
import com.xiwei.writerapp.data.NativeResult
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import org.json.JSONObject

class StatsController(
    private val activity: MainActivity,
    private val bridge: NativeCoreBridge,
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
            val todayStatsJson = bridge.getWritingStatsSummary(todayStr, todayStr)
            val weekStatsJson = bridge.getWritingStatsSummary(weekStartStr, todayStr)
            val projectStatsResult = bridge.getWritingStatsByProject(weekStartStr, todayStr)

            withContext(Dispatchers.Main) {
                tvTodayStats.text = formatSummary(todayStatsJson)
                tvWeekStats.text = formatSummary(weekStatsJson)
                tvProjectStats.text = formatProjectStats(projectStatsResult)
            }
        }
    }

    private fun formatSummary(jsonStr: String?): String {
        if (jsonStr.isNullOrEmpty()) return "加载失败"
        return try {
            val json = JSONObject(jsonStr)
            val chars = json.optInt("total_human_typed_chars", 0)
            val activeSeconds = json.optInt("total_active_seconds", 0)
            val duration = if (activeSeconds > 3600) {
                "${activeSeconds / 3600}时${(activeSeconds % 3600) / 60}分"
            } else {
                "${activeSeconds / 60}分"
            }
            "纯输入: $chars 字\n活跃时长: $duration"
        } catch (e: Exception) {
            "解析失败"
        }
    }

    private fun formatProjectStats(result: NativeResult<Any>): String {
        return when (result) {
            is NativeResult.Success -> {
                try {
                    // data from getWritingStatsByProject is mapped to Any, so it's likely a Map/List or we can just toString it and parse as JSON
                    val jsonStr = com.google.gson.Gson().toJson(result.data)
                    val json = JSONObject(jsonStr)
                    val projects = json.optJSONArray("projects")
                    if (projects != null && projects.length() > 0) {
                        val sb = java.lang.StringBuilder()
                        for (i in 0 until projects.length()) {
                            val p = projects.getJSONObject(i)
                            val title = p.optString("project_title", "未命名")
                            val chars = p.optInt("human_typed_chars", 0)
                            sb.append("${i + 1}. $title: $chars 字\n")
                        }
                        sb.toString().trim()
                    } else {
                        "暂无数据"
                    }
                } catch (e: Exception) {
                    "解析失败"
                }
            }
            is NativeResult.Error -> "加载失败: ${result.message}"
            else -> "未知状态"
        }
    }
}
