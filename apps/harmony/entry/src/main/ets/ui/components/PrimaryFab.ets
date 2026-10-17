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
// 玻璃表面（Issue #937 重做）：毛玻璃主体之外补两层纯绘制装饰，全部 hitTestBehavior(None)，
// 唯一可点击的仍然只有中间的 Button：
//   体积感表面层：极低不透明度的系统色圆形，让浅色/深色/纯色背景下都有稳定可见的玻璃体积感，
//     不构成实心底色、不遮毛玻璃。
//   方向性边缘高光层：独立圆形容器（裁剪为圆形）用 radialGradient 画出「高光带 + 衰减」的边缘
//     高光环，并带一档更弱的内侧高光；方向与强度取自固定的 UI 约定光源方向（左上）。
//     这是**静态方向渐变**，只能叫玻璃光泽，不是实时反射，也不会随屏幕内容自动追光。
//   #905 第八轮那层 BlendMode.DIFFERENCE 反色描边环已删除：它只对描边正下方的本地像素做差值
//   混合，没有方向与衰减，做不出按光源位置分布的亮暗分区；不留旧圈叠加，避免多套边框。
//
// 按压视效取 PressShadowType.BLEND_WHITE（15% 白叠加），不用官方示例里的 BLEND_GRADIENT：
//   BLEND_GRADIENT 按定义是「中心 85% 白 → 边缘 100% 白」的整面叠白，本仓库在真机上量到
//   整颗刷白、深色底上白色 + 号消失。按压高光必须让「+」仍看得清，所以取较轻的一档。
//   两者都是同一个官方枚举里的档位，如需换回改这一行即可。
//
// 动态受光与静态玻璃高光分开记录：真正「随邻近光源位置/强弱变化」的边缘受光当前**未实现**。
//   官方 hdsEffect.pointLight 需要显式光源节点与受光节点配对，而它在 API 26 SDK 元数据里是
//   非公开接口（publicApi 为空）；本仓库真机记录显示 ArkTS 侧可构造、能力检查通过，
//   但 native 对所有传法一律回 `HDS_hdsbase: [42]Wrong argument type. int32 expected.`
//   （最终定位见 platform/API_MATRIX.md 的 #937 取证记录）。按能力边界不接入，也不再提交
//   试验性调用；没有具体可配对光源时，不凭空新增永久发光节点假扮环境反射。
//   systemMaterial 的自动采样沉浸材质在内容流 FAB 上仍被官方范围拒绝（见上文 #888 记录）。
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
  /** 上一次记录的实测尺寸（vp），用于只在真实尺寸变化时上报几何事件 */
  private lastGeometryWidth: number = -1
  private lastGeometryHeight: number = -1

  // Issue #911：FAB 挂载起点。这是**代码所请求的效果**（透明底 + Thin 模糊 + 方向性渐变高光 +
  // 按压视效），不可命名为"材质渲染成功"——真机渲染结果仍需独立的 hilog.txt 佐证。
  aboutToAppear(): void {
    const fields: DiagnosticField[] = [
      { key: 'source', value: 'content_fab' },
      { key: 'visualStyle', value: 'blur_thin_gradient_edge_press_shadow' }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'ui.fab_mounted', 'PrimaryFab', fields)
  }

  // Issue #911：只在实测尺寸真正变化时上报几何事件，不在 build 每次重算时写。
  private recordGeometryChange(area: Area): void {
    const width: number = Number(area.width)
    const height: number = Number(area.height)
    if (width === this.lastGeometryWidth && height === this.lastGeometryHeight) {
      return
    }
    this.lastGeometryWidth = width
    this.lastGeometryHeight = height
    const fields: DiagnosticField[] = [
      { key: 'width', value: String(width) },
      { key: 'height', value: String(height) }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'ui.fab_geometry_changed', 'PrimaryFab', fields)
  }

  /** 共享触摸处理：按下点亮按压光并锁切页、移动判拖动、抬起/取消熄灭并解锁 */
  private handleTouch(event: TouchEvent): void {
    const touch = event.touches[0]
    if (event.type === TouchType.Down) {
      this.dragged = false
      this.downX = touch.x
      this.downY = touch.y
      this.pressShadowType = hdsEffect.PressShadowType.BLEND_WHITE
      PrimaryTabGestureLock.getInstance().setLocked(true)
      // Issue #911：按压进入预期状态（点亮 BLEND_WHITE）。Move 不记录。
      const downFields: DiagnosticField[] = [
        { key: 'state', value: 'down' },
        { key: 'effect', value: 'blend_white' }
      ]
      HarmonyDiagnosticEvents.record('Info', 'App', 'ui.fab_press_state', 'PrimaryFab', downFields)
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
    const upState: string = event.type === TouchType.Cancel ? 'cancel' : 'up'
    const upFields: DiagnosticField[] = [
      { key: 'state', value: upState },
      { key: 'effect', value: 'none' }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'ui.fab_press_state', 'PrimaryFab', upFields)
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
    // 玻璃三层：毛玻璃 Button（唯一可点击）→ 极低不透明度系统色表面层 → 方向性边缘高光层。
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

      // 体积感表面层：极低不透明度系统色，让浅/深/纯色背景下都有稳定可见的玻璃体积感；
      // 透明度很低，不构成实心底色，不遮毛玻璃；不拦触摸，可点击的始终只有 Button。
      Circle()
        .width(PRIMARY_FAB_SIZE_VP)
        .height(PRIMARY_FAB_SIZE_VP)
        .fill($r('sys.color.font_primary'))
        .opacity(0.06)
        .hitTestBehavior(HitTestMode.None)

      // 方向性边缘高光层：渐变圆心落在左上角外侧（约定的光源方向），高光带只扫过左上圆边、
      // 向内一档衰减，右下圆边不受光——静态方向渐变=玻璃光泽，不是实时反射/跟手追光。
      // 圆形容器 + clip 保证渐变被裁在圆内；不拦触摸，不叠第二套边框。
      Stack()
        .width(PRIMARY_FAB_SIZE_VP)
        .height(PRIMARY_FAB_SIZE_VP)
        .borderRadius(PRIMARY_FAB_SIZE_VP / 2)
        .clip(true)
        .radialGradient({
          // 光源方向：左上角外侧约 5% 处；渐变盘与左上圆边相交成月牙形高光。
          center: [PRIMARY_FAB_SIZE_VP * 0.05, PRIMARY_FAB_SIZE_VP * 0.05],
          radius: PRIMARY_FAB_SIZE_VP * 0.23,
          colors: [
            ['#00000000', 0.0],
            ['#00000000', 0.55],
            ['#1AFFFFFF', 0.62],
            ['#59FFFFFF', 0.80],
            ['#1AFFFFFF', 0.92],
            ['#00000000', 1.0]
          ],
          repeating: false
        })
        .hitTestBehavior(HitTestMode.None)
    }
    .width(PRIMARY_FAB_SIZE_VP)
    .height(PRIMARY_FAB_SIZE_VP)
    // Issue #911：根 Stack 实测尺寸变化时记录几何事件（只在真正变化时写）。
    .onAreaChange((oldArea: Area, newArea: Area) => { this.recordGeometryChange(newArea) })
  }
}
