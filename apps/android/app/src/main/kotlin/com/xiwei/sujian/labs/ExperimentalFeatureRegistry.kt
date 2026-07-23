package com.xiwei.sujian.labs

import com.xiwei.sujian.R

/**
 * 实验室功能注册表 — 可扩展的实验性功能管理
 *
 * 每个实验性功能定义为一个 ExperimentalFeature，
 * 通过 registry 统一管理，Compose SettingsRoute 根据注册表动态生成开关。
 */
data class ExperimentalFeature(
    val id: String,
    val titleRes: Int,       // R.string.xxx
    val summaryRes: Int?,    // R.string.xxx or null
    val defaultEnabled: Boolean,
    val scope: String        // "system_bars", "editor", "sync" etc.
)

object ExperimentalFeatureRegistry {
    val features = listOf(
        ExperimentalFeature(
            id = "fullscreen_immersive",
            titleRes = R.string.lab_fullscreen_immersive,
            summaryRes = R.string.lab_fullscreen_immersive_summary,
            defaultEnabled = false,
            scope = "system_bars"
        )
    )

    fun findById(id: String): ExperimentalFeature? = features.find { it.id == id }
}