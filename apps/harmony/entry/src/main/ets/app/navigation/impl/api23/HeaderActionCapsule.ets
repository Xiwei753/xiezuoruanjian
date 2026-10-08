// HeaderActionCapsule — 页头尾部动作的胶囊容器（API23+ HDS 标题栏自定义区域用）。
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
// 原先的 HdsHeaderActionMenu.ets（menu.value 适配器）已随三个壳一起删掉。
//
// 视觉与底栏胶囊对齐：
//   磨砂     沉浸式系统材质 systemMaterial（底栏走 HdsTabs 专属的 systemMaterialEffect，
//            同一套材质，surfaceType 不同）
//   描边     发丝白，只做轮廓保险
//
// 关于「手指按下的光源」：底栏那圈跟随手指的光来自 HdsTabs 专属属性 systemMaterialEffect
// （materialType 用 MaterialType.ADAPTIVE，官方示例值）。原生 menu.value 渲染的按钮挂上
// 标题栏的 systemMaterialEffect 后实测有光：按下时三个圆内部被点亮，按住的那个最亮、向两侧递减，
// 圆与圆之间的空隙不受影响 —— 材质是每按钮一块，不是整条标题栏一整片。
// 本组件是自绘节点，走通用属性 systemMaterial + uiMaterial.ImmersiveMaterial，
// 是否在生效范围内以实机日志（Material inactive: out of scope）为准，不要凭猜测定参数。
// 实机结论：有效，且胶囊与光同时成立。静止表面 20 级与底栏胶囊同值；
// 按住时从手指位置最亮向两侧递减（242 → 73）。
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
  @Param inHdsTitleBar: boolean = false
  @Local dragged: boolean = false

  private downX: number = 0
  private downY: number = 0

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

  build() {
    // Issue #880：systemMaterial(undefined) 在 ArkTS 中不合法，改为条件渲染两个 Row 分支。
    // inHdsTitleBar=true（手机 HDS 原生标题栏）：沉浸式系统材质，透明背景，无描边。
    // inHdsTitleBar=false（宽屏 WidePrimaryShell 内容流悬浮组）：真实底色 + 发丝描边，无 systemMaterial。
    if (this.inHdsTitleBar) {
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
      .onTouch((event: TouchEvent) => {
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
      })
    } else {
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
      .onTouch((event: TouchEvent) => {
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
      })
    }
  }
}
