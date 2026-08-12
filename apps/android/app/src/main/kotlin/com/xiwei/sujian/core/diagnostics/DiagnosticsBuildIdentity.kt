package com.xiwei.sujian.core.diagnostics

import com.xiwei.sujian.BuildConfig

/**
 * #623 评论 3：不可变构建身份 — 标识当前 APK 的版本、commit、flavor 和 buildType。
 *
 * 日志文件名按 [buildKey] 生成，versionCode/git sha/flavor/buildType 任一变化
 * 都会自然进入另一个日志文件。从 [BuildConfig] 生成的唯一入口确保运行时
 * 身份与 APK 打包元数据一致。
 */
data class DiagnosticsBuildIdentity(
    val versionName: String,
    val versionCode: Int,
    val gitCommitSha: String,
    val flavor: String,
    val buildType: String,
    val applicationId: String,
) {
    /**
     * 文件安全的构建 key，例如 `v1234-e2ce827-ai-debug`。
     * 用于日志文件名：sujian-current-v1234-e2ce827-ai-debug.log
     */
    val buildKey: String
        get() = "v$versionCode-$gitCommitSha-$flavor-$buildType"

    companion object {
        /**
         * 从当前 BuildConfig 生成 identity 的唯一入口。
         */
        fun fromBuildConfig(): DiagnosticsBuildIdentity =
            DiagnosticsBuildIdentity(
                versionName = BuildConfig.VERSION_NAME,
                versionCode = BuildConfig.VERSION_CODE,
                gitCommitSha = BuildConfig.GIT_COMMIT_SHA,
                flavor = BuildConfig.FLAVOR,
                buildType = BuildConfig.BUILD_TYPE,
                applicationId = BuildConfig.APPLICATION_ID,
            )
    }
}
