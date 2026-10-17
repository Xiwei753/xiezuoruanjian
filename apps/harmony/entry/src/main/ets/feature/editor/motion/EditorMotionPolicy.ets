// EditorMotionPolicy — 运动策略定义。
//
// Issue #879：定义写作区文字运动的策略参数。
// 参考 Android 的产品语义（协同/独立模式、减少动态效果）但不复制视觉实现。
//
// 从 LocalSettings 构造，把用户设置翻译成运动层的策略参数：
// - typingDurationMs：吐字/吞字动画时长（默认 100ms）。
// - cursorDurationMs：光标平滑移动时长（默认 80ms）。
// - mode：协同模式（文字和光标一起动画）或独立模式（分别动画）。
// - reduceMotion：系统级减少动态效果（无障碍），独立于用户动画开关。
//
// Issue #879 复核评论：reduceMotion 不应因 typing 和 cursor 都关就推成 true。
// reduceMotion 的语义是"系统级减少动态效果"（无障碍），不是"用户关了某个动画开关"。
// 各动画开关（typing/cursor/coordinated）独立判断，互不影响。
//
// 不修改 DTO 与 Rust，只读 LocalSettings。

import { LocalSettings } from '../../../corebridge/dto/SettingsDtos'

/**
 * 运动模式枚举。
 *
 * - Coordinated：协同模式——文字和光标在同一动画周期内一起运动，
 *   光标绑定裁切前沿 front，文字和光标同步推进/退回。
 * - Independent：独立模式——文字动画和光标动画分别进行，
 *   文字用裁切窗口，光标用独立的位置插值。
 */
export enum MotionMode {
  Coordinated = 'coordinated',
  Independent = 'independent'
}

/**
 * 运动策略。
 *
 * 封装从 LocalSettings 投影出的动画参数和模式判断。
 * 运动协调器（EditorMotionCoordinator）和规划器（EditorMotionPlanner）
 * 消费本策略决定是否启动动画、动画时长、协同/独立模式。
 *
 * Issue #879 复核评论修复：
 * - reduceMotion 独立于用户动画开关，语义为"系统级无障碍减少动态效果"。
 * - shouldAnimateText() 只检查 isTypingAnimationEnabled。
 * - shouldAnimateCaret() 只检查 isSmoothCursorEnabled。
 * - shouldUseCoordinatedMode() 只检查 isCoordinatedEnabled。
 */
export class EditorMotionPolicy {
  /** 吐字/吞字动画时长（ms） */
  readonly typingDurationMs: number
  /** 光标平滑移动时长（ms） */
  readonly cursorDurationMs: number
  /** 运动模式：协同或独立 */
  readonly mode: MotionMode
  /** 系统级减少动态效果（无障碍）——独立于用户动画开关 */
  readonly reduceMotion: boolean
  /** 是否启用吐字动画 */
  readonly isTypingAnimationEnabled: boolean
  /** 是否启用光标平滑移动 */
  readonly isSmoothCursorEnabled: boolean
  /** 是否启用协同模式（文字+光标一起动画） */
  readonly isCoordinatedEnabled: boolean

  constructor(
    typingDurationMs: number,
    cursorDurationMs: number,
    mode: MotionMode,
    reduceMotion: boolean,
    isTypingAnimationEnabled: boolean,
    isSmoothCursorEnabled: boolean,
    isCoordinatedEnabled: boolean
  ) {
    this.typingDurationMs = typingDurationMs
    this.cursorDurationMs = cursorDurationMs
    this.mode = mode
    this.reduceMotion = reduceMotion
    this.isTypingAnimationEnabled = isTypingAnimationEnabled
    this.isSmoothCursorEnabled = isSmoothCursorEnabled
    this.isCoordinatedEnabled = isCoordinatedEnabled
  }

  /**
   * 从 LocalSettings 构造运动策略。
   *
   * 映射规则：
   * - typingDurationMs ← editorTypingAnimationDurationMs（默认 100）
   * - cursorDurationMs ← editorSmoothCursorDurationMs（默认 80）
   * - mode ← editorCoordinatedTextCursorAnimationEnabled ? Coordinated : Independent
   * - isTypingAnimationEnabled ← editorTypingAnimationEnabled
   * - isSmoothCursorEnabled ← editorSmoothCursorEnabled
   * - isCoordinatedEnabled ← editorCoordinatedTextCursorAnimationEnabled
   * - reduceMotion：当前 Harmony 端没有独立的 reduceMotion 设置，设为 false。
   *   reduceMotion 的语义是"系统级无障碍减少动态效果"，不是"用户关了某个动画开关"。
   *   未来可从系统无障碍设置获取。
   */
  static fromSettings(settings: LocalSettings): EditorMotionPolicy {
    const isTypingAnimationEnabled: boolean = settings.editorTypingAnimationEnabled
    const isSmoothCursorEnabled: boolean = settings.editorSmoothCursorEnabled
    const isCoordinatedEnabled: boolean = settings.editorCoordinatedTextCursorAnimationEnabled
    // reduceMotion 独立于用户动画开关——当前 Harmony 端没有系统级 reduceMotion 设置
    const reduceMotion: boolean = false
    const mode: MotionMode = isCoordinatedEnabled ? MotionMode.Coordinated : MotionMode.Independent

    return new EditorMotionPolicy(
      settings.editorTypingAnimationDurationMs,
      settings.editorSmoothCursorDurationMs,
      mode,
      reduceMotion,
      isTypingAnimationEnabled,
      isSmoothCursorEnabled,
      isCoordinatedEnabled
    )
  }

  /**
   * 是否应该对文字（吐字/吞字）启动动画。
   * 只检查 isTypingAnimationEnabled——用户开关独立判断，不受 reduceMotion 影响。
   */
  shouldAnimateText(): boolean {
    return this.isTypingAnimationEnabled
  }

  /**
   * 是否应该对光标启动平滑移动动画。
   * 只检查 isSmoothCursorEnabled——用户开关独立判断，不受 reduceMotion 影响。
   */
  shouldAnimateCaret(): boolean {
    return this.isSmoothCursorEnabled
  }

  /**
   * 是否应该使用协同模式（文字+光标一起动画）。
   * 只检查 isCoordinatedEnabled。
   */
  shouldUseCoordinatedMode(): boolean {
    return this.isCoordinatedEnabled
  }

  /**
   * 是否有任何动画启用（文字或光标）。
   * 用于判断是否需要启动逐帧驱动。
   */
  shouldAnimate(): boolean {
    return this.shouldAnimateText() || this.shouldAnimateCaret()
  }
}
