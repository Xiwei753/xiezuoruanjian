// PrimaryFab — 全局统一的「主操作」悬浮按钮（圆形 + 号），固定在内容流右下角。
//
// 三个页面共用同一个组件：作品页（新建作品）、工作区页（新建卷）、星图页（新建星图）。
// 之前三处各自写 Button，背景色一个没设（系统蓝）、一个写死 themeConfig.colors.primary（纯黑），
// 才出现「作品页和星图页加号颜色不一样」。统一到这里以后配色只有一份。
//
// Issue #905：本组件维持**内容流右下角独立主操作按钮**的原始设计位置，不做后退。
//   #888 给内容流 Button 挂 .systemMaterial() 被系统拒绝，真机日志：
//   `Material inactive: out of scope. Use component in navigation title bar or Tabbar.`。
//   华为《开启沉浸光感》的公开生效范围只有 Navigation/NavDestination 标题栏与合规底部
//   TabBar，内容流右下角 FAB 不在范围内；换外层容器、改 modifier 顺序都不能解锁材质。
//   第二~五轮试过的「把 + 搬到 HDS 底部 MiniBar 承载光感」分支已按复核意见整体撤销：
//   不得为了拿不到的原生材质改变用户确定的按钮位置。
//
// 「拿不到沉浸材质」只否掉了 systemMaterial 这一条路，不等于内容流里没有任何可用的
// 模糊或光影能力。官方把通用背景模糊（backgroundBlurStyle / backgroundEffect）列为
// **独立**接口，与沉浸材质是否 active 无关；HDS 另有独立的按压视效入口
// hdsEffect.HdsEffectBuilder().pressShadow()。所以本轮恢复的是：
//   可透视毛玻璃：backgroundColor 取透明 + backgroundBlurStyle(BlurStyle.Thin)
//     —— 与底栏 barBackgroundBlurStyle 是同一套通用模糊（后者是 Tabs 对它的封装）。
//     纯实心底色会把模糊整个挡住，所以「透明底色」和「模糊」必须配套，不能只加一行 blur。
//   按压视效：pressShadow（见 build 里的 .visualEffect）。
//   两者都是**真实可用的普通能力**，但不是沉浸材质那种跟手追光：普通毛玻璃 ≠ IMMERSIVE，
//   不要写成已补回华为沉浸光感；该项在 #905 仍按未完成保留。
//
// 描边不再是写死的灰色 border，而是单独一层的「随背景反色」细圆环：
//   Stack { Button(毛玻璃主体) + Circle(仅描边).blendMode(BlendMode.DIFFERENCE, FAST) }。
//   DIFFERENCE 把描边像素与下层已绘制画面做差值混合，所以圆环颜色随按钮底下那圈画面
//   （毛玻璃透上来的邻近亮暗/色彩）变化，而不是一个固定灰环。它采样的是**描边覆盖处的
//   本地像素**，不会去屏幕别的坐标找远处光源，与 systemMaterial 的沉浸追光和
//   hdsEffect.pointLight 是三个不同机制；不要把它称作光感已复刻。
//   必须只把混色挂在描边 Circle 上：挂到 Button 主体会连正文图标和毛玻璃一起被混掉。
//   必须用 BlendApplyType.FAST：OFFSCREEN 模式会让同层的 backgroundBlurStyle 取不到正确画面。
//
// 按压视效取 PressShadowType.BLEND_WHITE（15% 白叠加），不用官方示例里的 BLEND_GRADIENT：
//   BLEND_GRADIENT 按定义是「中心 85% 白 → 边缘 100% 白」的整面叠白，本仓库在真机上量到
//   整颗刷白、深色底上白色 + 号消失。按压高光必须让「+」仍看得清，所以取较轻的一档。
//   两者都是同一个官方枚举里的档位，如需换回改这一行即可。
// hdsEffect.pointLight（点光源/受光）本轮**没有**接入，原因不是没查，而是查清后仍不可用：
//   d.ts 形参为 PointLightEffect { sourceType?, illuminatedType?, options?: { color?,
//   intensity?: number, height?: Dimension, bloom?: number } }；本仓库真机记录显示
//   ArkTS 侧构造成功、能力检查通过（apiLevel=26 hdsCore=true effect=true），但 native 层
//   对枚举成员、字面量整数、只留两字段、去掉 options 等各种传法一律回
//   `HDS_hdsbase: [42]Wrong argument type. int32 expected.`，即该实参结构在当前
//   SDK/设备上被 native 拒收。把已知被拒的调用塞回同一个 HdsEffectBuilder，会连累已经能
//   工作的 pressShadow（同一个 .visualEffect 只挂一次），因此本轮不接；结论与实机证据记在
//   platform/API_MATRIX.md，待有真机或官方修正 native 实参要求后再评估。
//
// 手势：按住时锁住底栏的横向滑动切页，避免按住加号横向拖动时页面被带走。
// 锁的实现见 app/navigation/PrimaryTabGestureLock.ets。

import { hdsEffect } from '@kit.UIDesignKit'
import { PRIMARY_FAB_SIZE_VP } from '../theme/LayoutTokens'
import { PrimaryTabGestureLock } from '../../app/navigation/PrimaryTabGestureLock'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'

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
  /** 按压视效档位：按下点亮，抬起/取消熄灭（默认 NONE 即无按压光） */
  @Local pressShadowType: hdsEffect.PressShadowType = hdsEffect.PressShadowType.NONE

  /** 共享触摸处理：按下点亮按压光并锁切页、移动判拖动、抬起/取消熄灭并解锁 */
  private handleTouch(event: TouchEvent): void {
    const touch = event.touches[0]
    if (event.type === TouchType.Down) {
      this.dragged = false
      this.downX = touch.x
      this.downY = touch.y
      this.pressShadowType = hdsEffect.PressShadowType.BLEND_WHITE
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
    // Up 与 Cancel 都走这里：发光和手势锁必须一起解除，否则手指移出后会残留高光。
    this.pressShadowType = hdsEffect.PressShadowType.NONE
    PrimaryTabGestureLock.getInstance().setLocked(false)
  }

  /** 共享点击处理：拖动过则吞掉，否则执行 onTap */
  private handleClick(): void {
    if (this.dragged) {
      this.dragged = false
      return
    }
    if (this.onTap) {
      // Issue #906：主操作「请求」事件记录在唯一真实点击入口（原 MiniBar 通道已撤销）。
      // 只在未拖动且确实调用 onTap 时记录；只记请求不记完成，业务成功与否由页面创建入口决定。
      const fields: DiagnosticField[] = [{ key: 'source', value: 'content_fab' }]
      HarmonyDiagnosticEvents.record('Info', 'User', 'ui.primary_action_requested', 'PrimaryFab', fields)
      this.onTap()
    }
  }

  build() {
    // 内容流位置：透明底色 + 通用背景模糊（毛玻璃）+ HDS 按压视效，外层叠一层只做反色混色的描边圆环。
    // stateEffect 置 false：按压反馈交给 pressShadow，避免系统内建按压态与它互相叠加。
    Stack({ alignContent: Alignment.Center }) {
      Button({ type: ButtonType.Circle, stateEffect: false }) {
        SymbolGlyph($r('sys.symbol.plus'))
          .fontSize(28)
          .fontColor([$r('sys.color.icon_primary')])
      }
      .width(PRIMARY_FAB_SIZE_VP)
      .height(PRIMARY_FAB_SIZE_VP)
      .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
      // 透明底色是毛玻璃的前提：实心色会盖在模糊层之上，模糊就看不见了。
      .backgroundColor(Color.Transparent)
      .backgroundBlurStyle(BlurStyle.Thin)
      // 阴影档位沿用实机校准结果（OUTER_DEFAULT_XS = 3 级灰度落差，与底栏胶囊一致）。
      .shadow(ShadowStyle.OUTER_DEFAULT_XS)
      .visualEffect(
        new hdsEffect.HdsEffectBuilder()
          .pressShadow(this.pressShadowType)
          .buildEffect()
      )
      .onTouch((event: TouchEvent) => { this.handleTouch(event) })
      .onClick(() => { this.handleClick() })

      // 只负责混色的细圆环：半透明白描边与下层画面做差值混合，得到随背景变化的反色边缘。
      // 直径取 SIZE-1，stroke 居中，外沿与 Button 圆边对齐；不拦触摸，可点击的始终只有 Button。
      Circle()
        .width(PRIMARY_FAB_SIZE_VP - 1)
        .height(PRIMARY_FAB_SIZE_VP - 1)
        .fill(Color.Transparent)
        .stroke('#66FFFFFF')
        .strokeWidth(1)
        .blendMode(BlendMode.DIFFERENCE, BlendApplyType.FAST)
        .hitTestBehavior(HitTestMode.None)
    }
    .width(PRIMARY_FAB_SIZE_VP)
    .height(PRIMARY_FAB_SIZE_VP)
  }
}
