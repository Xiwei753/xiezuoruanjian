# 输入动效设计方案 (Input Animation Design)

## 需求背景
为提升写作时的沉浸感和愉悦度，我们计划在编辑器中加入类似打字机或 Telegram 的顺滑输入反馈，包括：
1. 光标平滑向后移动的效果。
2. 新输入（或删除）的字符在光标附近有轻微淡入、滑出（或弹跃）的效果。

## 关键技术约束
在 Linux/Fedora KDE Wayland 环境下，尤其是使用 fcitx5 等中文输入法时，原生的 `TextField` 组件与输入法预编辑区域 (composing region) 之间的交互非常脆弱。

**绝对禁止的操作：**
- **重写或自定义底层 `TextField` 组件**：完全重新实现 Flutter 的 EditableText 会引入极高的维护成本，且极易破坏平台原生的文本选择、光标拖拽和快捷键支持。
- **拦截 IME 输入或修改 Composing Region**：如果在用户使用拼音等输入法输入阶段（预编辑框激活时）强行截断文本并替换为动画组件，会导致 fcitx5 输入框抽搐、无法正确上屏，甚至引起输入法崩溃。
- **破坏保存机制或 Markdown 正文内容**：动画仅限于视觉层，无论动画处于何种状态，不能影响实际数据的原子保存机制与 `chapter.md` 的内容一致性。
- **整页重建 (Rebuild)**：输入过程中的逐字更新绝不能触发整个 `WorkspaceScreen` 的 `setState`，否则会导致严重的性能问题（尤其是在长文本场景）。

## 设计方案：独立 Overlay 动画层

鉴于上述约束，我们选择采用 **无侵入的叠加层 (Overlay) 动画方案**。

### 1. 设置系统预留
已在 `SyncableSettings` 中预留以下开关，动画可全局关闭，默认关闭以保障性能：
- `inputAnimationEnabled`: 整体输入动效总开关。
- `typedCharacterAnimationEnabled`: 打字（输入与删除）时的字符弹跃或淡入出动效。
- `cursorAnimationEnhanced`: 光标平滑移动特效。

### 2. 动画工作原理思路
- **保持底层 TextField 稳定**：原有的 `EditorPanel` 和 `TextField` 保持不变，照常接收系统的 IME 输入，处理光标逻辑。
- **变化监听机制**：
  - 通过比对 `TextEditingController` 在不同帧之间的 `value.text` 与 `value.selection`，提取出**增量差异（新增的字符，或被删除的字符）**。
  - **忽略预编辑**：通过检查 `TextEditingValue.composing.isValid`，当处于拼音拼写阶段（预编辑状态有效）时，一律跳过不触发任何打字动画，防止干扰。
  - **过滤大段粘贴**：如果是粘贴进来的几十个字，不进行逐字播放，直接显示。
- **覆盖层渲染 (Overlay Rendering)**：
  - 使用 Flutter 的 `Overlay` 机制或 `Stack`，在 `TextField` 的确切位置之上覆盖一层透明的自定义画布。
  - 当确认有单字符级别的文本变化（插入或退格删除）时，利用 `TextPainter` 计算出该字符本应渲染在屏幕上的精确物理坐标 (X, Y)。
  - 在该坐标位置生成一个包含独立动画控制器的短暂 Widget，进行短暂的位移/透明度动画，动画结束后自动销毁。
  - 为实现“删除”时的字符吐出动画，可以在删除动作发生时，截获被删除的字符内容和原先的光标位置，在 Overlay 层播放动画即可。

### 3. 优势
- **100% 兼容输入法**：因完全不触碰原生的文本输入流程，因此 fcitx5 在 Wayland 下依然可以正常工作，预编辑框不会抽搐。
- **解耦与降级**：当用户关闭 `inputAnimationEnabled` 设置时，整个 Overlay 层和动画逻辑可以被直接忽略，零性能损耗退回原本的纯原生 `TextField`。
- **符合 Clean Architecture**：动画表现完全封装在 UI 表现层，`SettingsController` 提供配置状态，不污染任何 Controller 和 Repository。