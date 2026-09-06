package com.xiwei.sujian.storage.mirror

/*
 * MirrorPathUtils — 镜像文件路径与文件名净化工具。
 *
 * #649 评论 5560971132 修复 4：镜像路径从不可读的 `projects/<id>/volumes/<vid>/chapters/<cid>.md`
 * 改成用户可读的 `作品/<作品名>/<卷名>/<章节名>.md`。作品/卷/章节标题可能包含
 * 文件系统非法字符（`/ \ : * ? " < > |`）或全空白，需净化成安全文件名。
 *
 * ## 净化规则
 * - 空白或纯空格标题 → `untitled`
 * - 替换非法字符为 `_`
 * - 去掉末尾 `.` 和空格（Windows 不允许）
 * - 净化后若再次变空 → `untitled`
 *
 * 这些函数是纯函数，无副作用，可独立单测。
 */

/**
 * 把任意标题净化成安全的单级文件/目录名。
 *
 * - 空白返回 `untitled`。
 * - 替换 `/\:*?"<>|` 为 `_`。
 * - 去掉末尾 `.` 和空格（Windows 限制）。
 * - 净化后为空返回 `untitled`。
 */
fun sanitizeFileName(title: String): String {
    if (title.isBlank()) return "untitled"
    return title.trim()
        .replace(Regex("""[/\\:*?"<>|]"""), "_")
        .trimEnd('.', ' ')
        .ifBlank { "untitled" }
}

/**
 * 章节正文文件在镜像中的相对目录（相对 `Download/Sujian/`）。
 *
 * 结果形如 `作品/<净化作品名>/<净化卷名>`，不含文件名。
 * 调用方在此基础上拼接 [chapterFileName] 得到完整相对路径。
 */
fun chapterRelativeDir(
    projectTitle: String,
    volumeTitle: String,
): String = "作品/${sanitizeFileName(projectTitle)}/${sanitizeFileName(volumeTitle)}"

/**
 * 章节正文文件名：`<净化章节名>.md`。
 */
fun chapterFileName(chapterTitle: String): String = "${sanitizeFileName(chapterTitle)}.md"
