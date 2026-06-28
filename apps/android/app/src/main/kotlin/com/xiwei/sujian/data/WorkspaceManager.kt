package com.xiwei.sujian.data

import android.content.Context
import java.io.File

/**
 * WorkspaceManager — 工作区目录管理
 *
 * 负责工作区目录的获取。
 *
 * ## 架构定位
 * - 全局单例，管理工作区目录路径
 *
 * ## 职责边界
 * - **做**：获取工作区目录
 * - **不做**：初始化工作区目录、复制样例、拼装业务路径（全部由 Rust Core 负责）
 *
 * ## 使用场景
 * - data 层 legacy adapter 获取工作区路径
 */
object WorkspaceManager {
    fun getWorkspaceDir(context: Context): File {
        return File(context.filesDir, "workspace")
    }
}
