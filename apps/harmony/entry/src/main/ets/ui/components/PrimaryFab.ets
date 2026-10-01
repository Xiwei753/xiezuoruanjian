// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号）。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// 视觉走系统沉浸式系统材质（uiMaterial.ImmersiveMaterial），页面不自己拼效果：
//   形状：系统 Button（ButtonType.Circle），图标用系统符号 sys.symbol.plus。
//   前景色：sys.color.icon_primary。官方《组件适配沉浸光感》Button 一节明确要求
//        材质样式为 THIN / ULTRA_THIN 时 fontColor 用系统预定义的可反色颜色资源，
//        才会随材质自动反色；硬编码颜色不触发反色。
//   材质样式：THIN。官方 ImmersiveStyle 枚举说明把「悬浮按钮」列为 THIN / ULTRA_THIN
//        的推荐场景，并说明该样式对应材质的模糊程度与**高光效果**（即反射）。
//
// 摘掉 backgroundColor / backgroundBlurStyle / border / shadow 是官方硬性要求，不是洁癖：
//   这四个属性会盖在材质层之上，材质滤镜、折射、高光、阴影就全被挡住。
//   材质自己接管背景、边框、阴影，所以之前手写的半透明面和发丝描边全部撤掉，
//   不再叠第二套效果。阴影是唯一例外：官方允许关掉材质阴影后换回自定义 shadow
//   （见下面 applyShadow 那段），深色模式下的圆盘边界就靠它。
//
// 按压反馈：材质开了 lightEffect 之后，Button 内建的点击态/悬浮态反馈由材质接管，
//   所以 stateEffect 置 false，避免两套按压效果互盖（官方示例同样这么写）。
//
// 手势：按住时锁住底栏的横向滑动切页，避免按住加号横向拖动时页面被带走。
// 锁的实现见 app/navigation/PrimaryTabGestureLock.ets。
//
// 生效范围：官方《沉浸光感常见问题》写明沉浸式系统材质只在 Navigation/NavDestination
//   标题栏、横向 Tabs 中 barPosition 为 BarPosition.End 的底部 TabBar、弹窗类组件
//   以及 Slider/Toggle/Select 上生效。本组件挂在页面内容流，ArkUI 会打
//   `Material inactive: out of scope. Use component in navigation title bar or Tabbar.`
//   （改成挂外层容器是同一条日志，判定按位置不按组件类型）。
//   但这条日志与实际渲染不完全一致：实测内容流里的材质仍然响应（静止 0 级 → 按住 61 级），
//   不能只凭日志判定材质没生效，要以像素测量为准。
//   底栏那圈跟手的光另走 HdsTabs 专属属性 systemMaterialEffect，
//   见 app/navigation/impl/api23/HdsPrimaryTabsApi23.ets。
//   完整实测矩阵见 platform/API_MATRIX.md「沉浸式系统材质（uiMaterial.ImmersiveMaterial）」。

import { PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { PrimaryTabGestureLock } from '../../app/navigation/PrimaryTabGestureLock'
import { HarmonyMaterialService } from '../../platform/material/HarmonyMaterialService'

// 判定「按住又拖动」的位移阈值（vp）。低于它算点击，高于它算拖动。
const DRAG_SLOP_VP: number = 8

@ComponentV2
export struct PrimaryFab {
  /** 点击回调 */
  @Param onTap: () => void = () => {}
  /** 按住后是否已经拖动过（拖过就吞掉这次点击） */
  @Local dragged: boolean = false
  /** 按下位置（组件内坐标），用于判定是否拖动 */
  @Local downX: number = 0
  @Local downY: number = 0

  build() {
    // stateEffect 置 false：材质开了 lightEffect 后内建按压态会与材质的光感反馈互盖，
    // 官方《组件适配沉浸光感》的 Button 示例也是这么写的。
    Button({ type: ButtonType.Circle, stateEffect: false }) {
      SymbolGlyph($r('sys.symbol.plus'))
        .fontSize(28)
        .fontColor([$r('sys.color.icon_primary')])
    }
    .width(PRIMARY_FAB_SIZE_VP)
    .height(PRIMARY_FAB_SIZE_VP)
    .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
    // 背景色必须是 Transparent：官方说明开了沉浸光感后再设 backgroundColor 会盖住材质层。
    // 模糊、描边、投影同理，一律不设，材质自己接管。
    .backgroundColor(Color.Transparent)
    // applyShadow 传 false：官方《沉浸式系统材质视效》写明材质默认自带阴影且优先于
    // shadow 通用属性，要用自定义阴影必须先关掉材质阴影。这是官方唯一允许的替换通道 ——
    // border 没有。自定义 border 会盖在材质层上把高光挡掉，而材质自带的边框跟着
    // THIN 样式走，在深色模式纯黑底上几乎看不见（实测静止态表面 0 级，与背景同色）。
    // 所以边界改由投影承担，材质负责高光与按压光。
    .systemMaterial(HarmonyMaterialService.getInstance().floatingMaterial(false))
    .shadow(ShadowStyle.OUTER_DEFAULT_XS)
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
    .onClick(() => {
      if (this.dragged) {
        this.dragged = false
        return
      }
      if (this.onTap) {
        this.onTap()
      }
    })
  }
}
