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
//
// 圆盘边界分两层处理（Issue #811）：
//   内层 Button 只负责 ImmersiveMaterial + lightEffect（手指跟随光感），本体不画任何边。
//   外层单独做一个「边缘层」：1vp 淡描边兜住深色静止态的最低轮廓。
//   描边不压在 Button 的 systemMaterial 上，两个节点互不遮挡。
//   为什么不再用普通 shadow 承担边界：材质背景层在纯黑底上是全透明的，
//   没有可投的实体，自定义 shadow 一并消失（实测静止态 0 级、无任何渐变）。
//   Issue 里还建议给边缘层挂 hdsEffect 点光源的 DEFAULT_FEATHERING_BORDER 做动态反光，
//   但本机（OpenHarmony 7.0.0.105 / API 26）native 层每次都拒收该调用，
//   hilog 打 `HDS_hdsbase: [42]Wrong argument type. int32 expected.`，
//   且不出任何高光。实测证据记在 platform/API_MATRIX.md，因此这里只保留描边。
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
    Stack() {
      // 边缘层：深色静止态的最低轮廓，1vp 淡描边（实测灰度 38，与 #22FFFFFF 的 13% 白相符）。
      // 单独一层、不压在 Button 上，两个节点互不遮挡，材质层不会被挡掉。
      // hitTestBehavior(None)：这一层只画边不吃触摸，按钮的点击与手势锁不受影响。
      Row()
        .width(PRIMARY_FAB_SIZE_VP)
        .height(PRIMARY_FAB_SIZE_VP)
        .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
        .border({ width: 1, color: '#22FFFFFF' })
        .hitTestBehavior(HitTestMode.None)

      // 本体：只负责 ImmersiveMaterial + lightEffect，手指跟随光感由材质出，本体不画任何边。
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
      .backgroundColor(Color.Transparent)
      // applyShadow 传 true：阴影交回材质自己出，不再用普通 shadow 补边界。
      .systemMaterial(HarmonyMaterialService.getInstance().floatingMaterial(true))
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
}
