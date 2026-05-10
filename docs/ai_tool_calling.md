
### DeepSeek Thinking Mode + Tool Calls 注意事项

由于 DeepSeek API 在使用 Thinking Mode（深度思考模式）时，对工具调用（Tool Calls）有特殊要求，本项目在底层做出了特定的兼容处理：

- **tool_calls 场景下 reasoning_content 必须完整回传**：当 assistant 产生工具调用请求时，其返回的 `reasoning_content` 必须在随后的请求中原样附带回传，否则会触发 DeepSeek API 返回 400 错误。
- **普通无工具调用对话不应该把旧 reasoning_content 带回**：如果 assistant 没有产生任何工具调用，而是直接输出了回复结果，此时之前的 `reasoning_content` 已经完成使命，在未来的多轮对话上下文中会自动剔除（丢弃），不再回传以节省 tokens 并且避免造成上下文污染。
- 本项目通过 **DeepSeekMessageSerializer** 和 **AIConversationSession** 实现针对 provider 特殊行为的隔离与适配：
  - `DeepSeekMessageSerializer` 负责根据 assistant 是否存在 `tool_calls` 以及提供商是否为 DeepSeek 来动态决定是否序列化保留 `reasoning_content`。
  - `AIConversationSession` 用于记录会话上下文，如果触发工具调用，它标记 `requiresReasoningContentEcho = true` 来帮助系统进行流转控制。
- `reasoning_content` 是属于提供商在生成结果过程中的隐藏计算过程数据：
  - **不展示给用户**（UI 不应把它当成普通的 `content` 处理）。
  - **不写入正文**（它不是用户的写作数据，不可进入 `chapter.md` 等文档）。
  - **不写入日志**（考虑可能会占用较多内存和控制台空间）。
  - 需要持久化记录时只能进入 `app-meta/ai/traces/` 目录下的跟踪文件。
