# Bridge Contract

跨端调用必须遵守以下边界：

- Rust Core 是唯一业务事实来源，Android/Linux Bridge 只做类型转换和错误传播。
- UI/ViewModel/QML 不直接解析内部业务 JSON，不自行判断工作区、章节保存、写作事件分类或字数规则。
- 旧 `*_json` / `NativeCoreBridge` JSON 包装只用于兼容、调试、导入导出和迁移期适配，新调用应进入领域 Bridge。
- Bridge 错误必须包含稳定 `code` 和可展示 `message`，不能只依赖字符串匹配。

关键领域接口：

- Workspace：作品、卷、章节列表与创建。
- Writing：`openChapter`、`saveChapterContent`、`clearChapterContent`、`calculateWordCount`、`processWritingEvent`。
- Stats：项目统计和写作统计刷新/查询。
- StarMap：星图列表、创建、读取图、基础节点/边和布局操作。

章节保存语义：

- 普通保存必须走 Core 的验证保存，误传空字符串覆盖非空正文会返回 `EMPTY_OVERWRITE_BLOCKED`。
- 明确清空必须走专用 clear 接口，并返回保存回执。
- 正文始终为纯文本，Bridge 不得引入 HTML 保存路径。
