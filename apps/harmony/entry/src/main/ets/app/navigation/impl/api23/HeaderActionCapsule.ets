// HeaderActionCapsule — 页头尾部动作的胶囊容器。
//
// 背景：HDS 标题栏的 menu.value 每一项都是独立的圆形面，三项之间是断开的，
// 没有「容器」概念可套一个整体外框 —— HdsNavigationIconItemStyle 只有
// backgroundColor / iconColor / textColor 三个颜色字段，没有圆角、没有外层形状。
// 底栏那种「一个胶囊里横排若干图标」的观感，menu 渲染不出来。
// 官方为此留的口子是 titleBar.content.stackBuilder（6.0.0(20) 起），
// 允许自绘标题栏区域，本组件就是填进那个口子的。
//
// 动作来源没有变：仍是 Core ScreenPolicy 的 ActionSlot[] → sortedHeaderTrailingActions
// → NavigationHeaderActionDispatcher，这里只替换「怎么画」，不替换「做什么、谁决定顺序」。
//
// Issue #888：三个渲染分支：
//   1. inHdsTitleBar=true（HDS 原生标题栏 stackBuilder 区域）：
//      systemMaterial 挂在整棵 Row 上，这是标题栏特殊区域的写法，保留不变。
//   2. inHdsTitleBar=false + API 26 沉浸材质支持（内容流悬浮 Header）：
//      每个按钮自己挂 Button.systemMaterial，跟手光由真实 Button 材质提供。
//      不能把 systemMaterial 挂在外层 Row 上——内容流里 Row 上的 systemMaterial
//      会打 "Material inactive: out of scope"。
//   3. inHdsTitleBar=false + 不支持沉浸材质：
//      系统表面色 + 发丝描边，无 systemMaterial。
//
// 手势：按住胶囊任意位置（含按钮之间空隙与内边距）不得触发一级 Tabs 的横向滑动切页，
// 与 PrimaryFab 共用 PrimaryTabGestureLock。

import { ActionSlot, ActionRole } from '../../../../corebridge/dto/PlatformDtos'
import { sortedHeaderTrailingActions } from '../../NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from '../../NavigationHeaderActionDispatcher'
import { PrimaryTabGestureLock } from '../../PrimaryTabGestureLock'
import { HarmonyMaterialService } from '../../../../platform/material/HarmonyMaterialService'

/** 单个动作按钮的边长（vp），与设计稿 40vp 一致 */
const ACTION_BUTTON_SIZE_VP: number = 40
/** 按钮之间的间距（vp） */
const ACTION_GAP_VP: number = 4
/** 胶囊内边距（vp） */
const CAPSULE_PADDING_VP: number = 4
/** 判定为拖动（而非点击）的位移阈值（vp） */
const DRAG_SLOP_VP: number = 8

@ComponentV2
export struct HeaderActionCapsule {
  /** Core ScreenPolicy 下发的页头动作 */
  @Param actionSlots: ActionSlot[] = []
  /** 是否在 HDS 原生标题栏的 stackBuilder 区域内 */
  @Param inHdsTitleBar: boolean = false
  @Local dragged: boolean = false
  /** 设备是否支持沉浸式系统材质（API 26+ 且 isImmersiveMaterialSupported） */
  @Local materialSupported: boolean = false

  private downX: number = 0
  private downY: number = 0

  aboutToAppear() {
    this.materialSupported = HarmonyMaterialService.getInstance().floatingMaterial() !== undefined
  }

  /** 拖过之后本次手势不再算点击，避免横向拖动时误触发搜索/设置。 */
  private consumeClickIfDragged(): boolean {
    if (!this.dragged) {
      return false
    }
    this.dragged = false
    return true
  }

  private iconOf(slot: ActionSlot): Resource {
    if (slot.role === ActionRole.Sync) {
      return $r('app.media.ic_refresh')
    }
    if (slot.role === ActionRole.Search) {
      return $r('app.media.ic_search')
    }
    return $r('app.media.ic_settings')
  }

  private labelOf(slot: ActionSlot): string {
    if (slot.role === ActionRole.Sync) {
      return '同步'
    }
    if (slot.role === ActionRole.Search) {
      return '搜索'
    }
    return '设置'
  }

  /** 共享触摸处理：按下锁切页、移动判拖动、抬起解锁 */
  private handleTouch(event: TouchEvent): void {
    const touch = event.touches[0]
    if (event.type === TouchType.Down) {
      this.dragged = false
      this.downX = touch.x
      this.downY = touch.y
      PrimaryTabGestureLock.getInstance().setLocked(true)
      event.stopPropagation()
      return
    }
    if (event.type === TouchType.Move) {
      if (Math.abs(touch.x - this.downX) > DRAG_SLOP_VP ||
        Math.abs(touch.y - this.downY) > DRAG_SLOP_VP) {
        this.dragged = true
      }
      return
    }
    PrimaryTabGestureLock.getInstance().setLocked(false)
  }

  build() {
    if (this.inHdsTitleBar) {
      // ── 分支 1：HDS 原生标题栏 stackBuilder 区域 ──
      // systemMaterial 挂在整棵 Row 上，这是标题栏特殊区域的写法。
      Row() {
        ForEach(sortedHeaderTrailingActions(this.actionSlots), (slot: ActionSlot) => {
          Button({ type: ButtonType.Normal }) {
            Image(this.iconOf(slot))
              .width(24)
              .height(24)
              .fillColor($r('sys.color.icon_primary'))
          }
          .width(ACTION_BUTTON_SIZE_VP)
          .height(ACTION_BUTTON_SIZE_VP)
          .borderRadius(ACTION_BUTTON_SIZE_VP / 2)
          .backgroundColor(Color.Transparent)
          .stateEffect(false)
          .accessibilityText(this.labelOf(slot))
          .onClick(() => {
            if (this.consumeClickIfDragged()) {
              return
            }
            NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
          })
        }, (slot: ActionSlot) => `${slot.region}_${slot.role}`)
      }
      .alignItems(VerticalAlign.Center)
      .padding({ left: CAPSULE_PADDING_VP, right: CAPSULE_PADDING_VP + ACTION_GAP_VP })
      .borderRadius(ACTION_BUTTON_SIZE_VP / 2 + CAPSULE_PADDING_VP)
      .backgroundColor(Color.Transparent)
      .systemMaterial(HarmonyMaterialService.getInstance().floatingMaterial())
      .onTouch((event: TouchEvent) => { this.handleTouch(event) })
    } else if (this.materialSupported) {
      // ── 分支 2：内容流悬浮 Header + API 26 沉浸材质 ──
      // Issue #888：每个 Button 自己挂 systemMaterial，跟手光由真实 Button 材质提供。
      // 不能挂在外层 Row 上——内容流里 Row 上的 systemMaterial 会打 "Material inactive: out of scope"。
      Row() {
        ForEach(sortedHeaderTrailingActions(this.actionSlots), (slot: ActionSlot) => {
          Button({ type: ButtonType.Normal, stateEffect: false }) {
            Image(this.iconOf(slot))
              .width(24)
              .height(24)
              .fillColor($r('sys.color.icon_primary'))
          }
          .width(ACTION_BUTTON_SIZE_VP)
          .height(ACTION_BUTTON_SIZE_VP)
          .borderRadius(ACTION_BUTTON_SIZE_VP / 2)
          .backgroundColor(Color.Transparent)
          .systemMaterial(HarmonyMaterialService.getInstance().floatingMaterial())
          .accessibilityText(this.labelOf(slot))
          .onClick(() => {
            if (this.consumeClickIfDragged()) {
              return
            }
            NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
          })
        }, (slot: ActionSlot) => `${slot.region}_${slot.role}`)
      }
      .alignItems(VerticalAlign.Center)
      .padding({ left: CAPSULE_PADDING_VP, right: CAPSULE_PADDING_VP + ACTION_GAP_VP })
      .borderRadius(ACTION_BUTTON_SIZE_VP / 2 + CAPSULE_PADDING_VP)
      .backgroundColor(Color.Transparent)
      .onTouch((event: TouchEvent) => { this.handleTouch(event) })
    } else {
      // ── 分支 3：内容流悬浮 Header + 不支持沉浸材质 ──
      // 系统表面色 + 发丝描边，无 systemMaterial。
      Row() {
        ForEach(sortedHeaderTrailingActions(this.actionSlots), (slot: ActionSlot) => {
          Button({ type: ButtonType.Normal }) {
            Image(this.iconOf(slot))
              .width(24)
              .height(24)
              .fillColor($r('sys.color.icon_primary'))
          }
          .width(ACTION_BUTTON_SIZE_VP)
          .height(ACTION_BUTTON_SIZE_VP)
          .borderRadius(ACTION_BUTTON_SIZE_VP / 2)
          .backgroundColor(Color.Transparent)
          .stateEffect(false)
          .accessibilityText(this.labelOf(slot))
          .onClick(() => {
            if (this.consumeClickIfDragged()) {
              return
            }
            NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
          })
        }, (slot: ActionSlot) => `${slot.region}_${slot.role}`)
      }
      .alignItems(VerticalAlign.Center)
      .padding({ left: CAPSULE_PADDING_VP, right: CAPSULE_PADDING_VP + ACTION_GAP_VP })
      .borderRadius(ACTION_BUTTON_SIZE_VP / 2 + CAPSULE_PADDING_VP)
      .backgroundColor($r('sys.color.comp_background_gray'))
      .borderWidth(0.5)
      .borderColor($r('sys.color.icon_secondary'))
      .onTouch((event: TouchEvent) => { this.handleTouch(event) })
    }
  }
}
