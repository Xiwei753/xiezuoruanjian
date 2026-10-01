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
//   内层 Button 保留 ImmersiveMaterial + lightEffect，本体不画任何边。
//     注意：本机（OpenHarmony 7.0.0.105 / API 26）内容流里材质实际没有渲染出来，
//     静止和按住都是 0 级，这两个属性是为系统能力变化 / 组件进入受支持范围后
//     能接上系统材质而保留的，不是当前深色边界的成因。
//   外层单独做一个「边缘层」：1vp 淡描边兜住深色静止态的最低轮廓。
//     本轮真正解决深色模式可见性的是这一圈描边，不是内容流里的 lightEffect。
//   描边作为独立节点绘制，不把 border modifier 挂到 systemMaterial 的 Button 本体上；
//   边缘层 zIndex(1)，始终优先保证 1vp 最低轮廓可见。
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
//   但本轮实测已经确认：内容流里的 systemMaterial 并没有真正渲染出来，
//   静止态和按住态都是 0 级，ArkUI 这条 out of scope 日志是准的。
//   （之前一度报过「静止 0 级 → 按住 61 级」，那是测量假象：对话框遮罩把整屏抬亮、
//   以及 uinput 按下未配对抬起导致组件停在按压态。抓帧纪律见 API_MATRIX。）
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
      // 作为独立节点绘制，border 不挂到 systemMaterial 的 Button 本体上；zIndex(1) 保证
      // 这圈轮廓始终画在 Button 之上，不依赖材质是否渲染成功。
      // hitTestBehavior(None)：这一层只画边不吃触摸，按钮的点击与手势锁不受影响。
      Row()
        .width(PRIMARY_FAB_SIZE_VP)
        .height(PRIMARY_FAB_SIZE_VP)
        .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
        .border({ width: 1, color: '#22FFFFFF' })
        .hitTestBehavior(HitTestMode.None)
        // 明确提到 Button 之上：这圈描边的职责是「无论材质是否工作都保证轮廓可见」，
        // 不能依赖当前系统恰好不渲染材质。Stack 默认后加入的 Button 在上层，
        // 一旦系统放开内容流材质，Button 会把底下的兜底描边压住。Button 保持默认层级即可。
        .zIndex(1)

      // 本体：保留 ImmersiveMaterial + lightEffect，本体不画任何边。
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
