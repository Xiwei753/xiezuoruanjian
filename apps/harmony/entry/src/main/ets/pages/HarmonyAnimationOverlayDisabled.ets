/**
 * HarmonyAnimationOverlayDisabled.ets
 *
 * 当前 Harmony 动画禁用，等待桌面/Android 稳定后再实现。
 *
 * 第一版 Harmony 目标：能写、能保存、能滚动、能切换章节。
 * 不测动画。
 *
 * 当前保留此文件仅作为参考，避免后续 agent 误以为动画已可用。
 * MainWorkspace.ets / WritingPage.ets 中 typingAnimationEnabled / smoothCursorEnabled 强制 false，
 * 不会渲染任何 overlay。
 */

// ── 插入动画 overlay（已禁用）──
// 原始代码位于 MainWorkspace.ets EditorArea() 的 Stack 内：
//
// if (this.typingAnimationEnabled && this.showInsertAnimation) {
//   Text(this.lastInsertedText)
//     .fontSize(this.settings.editorFontSize)
//     .fontColor(this.themeConfig.colors.primary)
//     .opacity(0.6)
//     .position({ x: this.insertAnimX, y: this.insertAnimY })
//     .scale({ x: 1.1, y: 1.1 })
//     .animation({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseOut })
// }

// ── 删除动画 overlay（已禁用）──
//
// if (this.typingAnimationEnabled && this.showDeleteAnimation) {
//   Text(this.deleteAnimText)
//     .fontSize(this.settings.editorFontSize)
//     .fontColor(this.themeConfig.colors.textPrimary)
//     .opacity(this.deleteAnimOpacity)
//     .position({ x: this.deleteAnimX, y: this.deleteAnimY })
//     .scale({ x: this.deleteAnimScale, y: this.deleteAnimScale })
//     .translate({ x: this.deleteAnimTranslateX, y: this.deleteAnimTranslateY })
//     .animation({ duration: this.settings.editorTypingAnimationDurationMs || 150, curve: Curve.EaseIn })
// }

// ── 自定义光标 overlay（已禁用）──
//
// if (this.smoothCursorEnabled) {
//   Column()
//     .width(2)
//     .height(this.settings.editorFontSize * 1.2)
//     .backgroundColor(this.themeConfig.colors.primary)
//     .opacity(this.cursorBlinkOpacity)
//     .position({ x: this.cursorX, y: this.cursorY })
//     .animation({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut })
// }

// ── onContentChange 中的动画逻辑（已禁用）──
//
// 插入动画逻辑：
// if (this.typingAnimationEnabled && changed.insertedText.length > 0 && changed.insertedText.length <= 3 && !this.isScrolling) {
//   const caretOffset = this.textAreaController.getCaretOffset()
//   this.lastInsertedText = insertedText
//   this.insertAnimX = caretOffset.x
//   this.insertAnimY = caretOffset.y - this.scrollOffsetY
//   this.showInsertAnimation = true
//   this.insertAnimTimer = setTimeout(() => { this.showInsertAnimation = false }, durationMs)
// }
//
// 删除动画逻辑：
// if (this.typingAnimationEnabled && changed.deletedText.length > 0 && changed.deletedText.length <= 3 && !this.isScrolling) {
//   const caretOffset = this.textAreaController.getCaretOffset()
//   this.deleteAnimText = deletedText
//   this.deleteAnimX = caretOffset.x
//   this.deleteAnimY = caretOffset.y - this.scrollOffsetY
//   this.deleteAnimTranslateX = deletedText.length * this.settings.editorFontSize * 0.6
//   animateTo({ duration: durationMs, curve: Curve.EaseIn }, () => {
//     this.deleteAnimTranslateX = 0
//     this.deleteAnimScale = 0.5
//     this.deleteAnimOpacity = 0
//   })
// }
//
// 平滑光标逻辑：
// if (this.smoothCursorEnabled && !this.isScrolling) {
//   const caretOffset = this.textAreaController.getCaretOffset()
//   animateTo({ duration: this.settings.editorSmoothCursorDurationMs || 80, curve: Curve.EaseOut }, () => {
//     this.cursorX = caretOffset.x
//     this.cursorY = caretOffset.y - this.scrollOffsetY
//   })
// }
