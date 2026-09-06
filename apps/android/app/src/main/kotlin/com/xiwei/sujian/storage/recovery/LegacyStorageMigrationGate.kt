package com.xiwei.sujian.storage.recovery

import android.content.Context
import java.io.File

/**
 * LegacyStorageMigrationGate — 旧版存储迁移门控。
 *
 * 只判断旧版 `filesDir/sujian-git/workspace` 结构是否存在，不触碰 Core、
 * 不依赖 [com.xiwei.sujian.core.interop.app.AppServiceBridge]、不初始化任何
 * Repository。让 [com.xiwei.sujian.app.MainActivity] 在启动时能以最低代价决定
 * 是先显示迁移入口还是直接进入 SujianApp（Issue #649 评论 5559763924）。
 *
 * ## 为什么放在 `:app` 的 `storage/recovery` 包
 *
 * 本类只读 [Context.getFilesDir]，不依赖 Core/UniFFI，理论上可放 `:core:platform`。
 * 但它只服务于 app 启动门控，放 `:app` 方便 MainActivity 引用，避免向 `:core:platform`
 * 引入"旧版迁移"语义（platform 层只描述当前私有根，不关心历史结构）。
 *
 * ## 安全约束
 *
 * 只读 [File.exists]，不创建/删除任何文件，不持有状态。
 */
object LegacyStorageMigrationGate {
    private const val LEGACY_GIT_WORKSPACE_PATH = "sujian-git/workspace"

    /**
     * 旧版 `filesDir/sujian-git/workspace` 是否存在。
     *
     * 返回 true 表示应用从旧版本升级且尚未迁移，调用方应先显示迁移入口，
     * 迁移成功后再调用 [com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot.ensure]
     * 并初始化 WriterAppService。返回 false 表示新安装或已迁移，可正常进入 UI。
     */
    fun legacyGitWorkspaceExists(context: Context): Boolean =
        File(context.filesDir, LEGACY_GIT_WORKSPACE_PATH).exists()
}
