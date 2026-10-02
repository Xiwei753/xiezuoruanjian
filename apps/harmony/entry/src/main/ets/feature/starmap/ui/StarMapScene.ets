// ─── StarMapScene.ets ───
// 可递归星图场景组件：负责某一张星图的完整显示和交互。
// 可嵌套在 StarMapScreen（根场景）或 StarMapEmbedScene（子星图 Embed 内部）中。
//
// 关键约束：
// 1. 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath
// 2. 所有 bridge mutation 调用使用 this.starmapId（当前 scene 的 starmapId）
// 3. Embed 的 title/border 区域负责选中 Embed；innerContent 区域让事件穿透到 child StarMapScene
// 4. 递归结构：StarMapScene → StarMapEmbedScene → child StarMapScene
// 5. 菜单和对话框都不属于 Scene 的 Stack：菜单用 ArkUI 程序化 bindMenu（由系统定位、自动避让屏幕边缘），
//    对话框用 openCustomDialog 挂到窗口层级。否则菜单会被圆形 Embed 的 clip(true) 裁掉（Issue #813）。
// 6. bindMenu 按"被绑定组件"定位，所以菜单不能直接挂在全屏背景层或节点本体上：
//    Scene 栈里只放 1vp×1vp 的透明锚点，锚点位置在长按时记成手指的屏幕坐标，
//    这样菜单既贴着手指出现，又不会被父级 clip 裁掉（Issue #813 复审）。
// 7. 菜单必须接 onDisappear 把显示标记写回状态：程序化 bindMenu 的 isShow 不会自动双向同步，
//    不写回的话第二次长按同一对象会因为状态没变化而弹不出来（Issue #813 复审）。
// 8. Embed 的 title / 圆环由外层 Stack 的 onTouchIntercept 动态分流，不再用三根矩形窄条：
//    矩形条被 clip(true) 裁出来的只是圆内几段直线，斜着点边框会掉进 child Scene（Issue #813 复审）。
// 9. 圆形子视图必须同时声明圆形热区（responseRegion）并用 HitTestMode.Block 独占 title / 圆环：
//    组件默认热区是整个矩形，borderRadius + clip 只裁视觉不裁热区；HitTestMode.Default 是
//    "自己和子节点都参加"，父 Embed 和 child Scene 会一起进手势竞争（Issue #813 复审）。
// 10. 预览连线的起点和正式边共用 geometry 的 edgeEndpointBoundaryPoint：
//     预览从对象边界出发，不要从中心出发，否则松手前后会跳变。
// 11. 双指缩放挂在**根 Scene 的最外层 Stack**（统一手势入口），不再挂在最底下那个空白 Column：
//     空白 Column 只是 Stack 里的一个兄弟节点，手指一旦落在子星图上就不会经过它，
//     于是"一指在子图上、一指在空白上"时双指缩放直接失效（#816 真机症状）。
//     祖先节点的 onTouch 一定收得到子节点的触摸，所以在根 Stack 上监听就覆盖了所有落点。
// 12. 双指只有一台**全局相机**（#818）：整棵递归星图一起缩放，缩放目标不再由
//     "两指中心落在哪一层"决定。手指落在节点上、落在子星图上、落在二层子星图上，
//     结果完全一样。旧方向"两指中心递归命中 → 找最深 ownerScenePath → 缩放那个 Scene"已作废。
// 13. 变换只有两种（#818）：根 Scene 的全局相机（用户可改），和每个子 Scene 的
//     局部适配 fitScale + 居中偏移（布局用，用户改不了）。禁止再用"每深一层乘 0.6"
//     这种写死系数，也禁止在子层再存一份用户缩放倍数。
// 14. 选中态全树只有一份（StarMapSelectionState），身份是 scenePath + kind + itemId。
//     每层各存一份 selectedNodeId 的话，父子两层会同时亮，且分不清选中的是哪一层的哪个东西。
// 15. 选中、connect 松手、菜单归属全部走 geometry 的递归命中一个入口，
//     不再各写一份"点在不在这一层"的判断；双指缩放不走它（#818：相机只有一台）。

import hilog from '@ohos.hilog'
import { ComponentContent } from '@kit.ArkUI'
import { CreateTitleDialogArgs, createTitleDialogContent } from './CreateTitleDialog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import {
  StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed,
  StarMapTargetPath, StarMapPoint, StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  PlatformNodeRect, PlatformEdgeRender,
  RecursiveSceneContext, RecursiveSceneSource, RecursiveHit,
  deriveNodeRectFromLayoutNode,
  computeAllEdgeRenders,
  screenToCanvas, canvasToScreen, resolveEdgeEndpointAnchor,
  edgeEndpointBoundaryPoint, buildCircleResponseBands,
  buildRecursiveSceneContext, resolveRecursiveHit, convertPointToRoot,
  buildTargetPathForHost, planCrossLayerEdge, describeScenePath,
  EMBED_TITLE_HIT_HEIGHT, EMBED_BORDER_HIT_WIDTH
} from '../platform/StarMapGeometry'
import {
  PlatformLayoutNode, PlatformPoint,
  buildFreeformLayout, buildEmbedLayoutNodes, applyFreeformLayout,
  DEFAULT_EMBED_DIAMETER
} from '../platform/StarMapLayout'
import {
  PlatformMotionConfig,
  wobbleXFromProgress, wobbleYFromProgress,
  MotionPauseTracker
} from '../platform/StarMapMotion'
import { GestureState, StarMapGestureMode, StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import { StarMapSelectionState } from '../platform/StarMapSelectionState'
import {
  StarMapSceneNode, StarMapSceneRegistry, Viewport,
  ContentBounds, computeContentBounds, computeFittedViewport, computeCenteredOffset,
  computeCameraPinch, GlobalCameraTransform,
  CIRCLE_INNER_SAFE_RATIO
} from '../platform/StarMapViewport'
import {
  logPinchBegin, logPinchEnd, logRecursiveHit, logViewportState, logCreateResult,
  logSelectionChanged, logConnectBegin, logConnectEnd
} from '../platform/StarMapDiagnostics'
import { StarMapEmbedScene } from './StarMapEmbedScene'

/** wobble 视觉偏移（dx, dy）：只影响显示，不写回 Core。 */
interface WobbleOffset {
  dx: number
  dy: number
}



// ─── Embed 命中区尺寸 ───
// EMBED_TITLE_HIT_HEIGHT / EMBED_BORDER_HIT_WIDTH 直接 import 自 StarMapGeometry，
// 不在本文件再抄一份。命中测试和真实触摸必须用同一份数值：曾经几何用 3vp、
// UI 用 4vp，于是手指按这一圈会判成外层边框，而把线拖到同一点松手却判成子图内部。

/**
 * 星图选中描边色。普通节点和 Embed 共用这一个值。
 * 不能用 themeConfig.colors.primary：主题主色跟着配色走，深色主题下可能和
 * 子星图外壳底色接近，选中描边会糊成"边贴边"，选中态直接消失（Issue #813 复审、#818）。
 */
const STARMAP_SELECTED_STROKE_COLOR: string = '#FF6B6B'

// ─── 视口常量 ───

/** 子图内容适配到 Embed 圆形可用区时，四边留白（vp）。 */
const EMBED_FIT_PADDING_VP: number = 8

/** 两指中心：全局相机的缩放锚点就是它（#818）。 */
function twoFingerCenter(a: PlatformPoint, b: PlatformPoint): PlatformPoint {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
}

@Component
export struct StarMapScene {
  // ─── @Prop：从父组件只读传入 ───
  /** 根星图 ID（整个 StarMapScreen 生命周期内不变） */
  @Prop rootStarmapId: string = ''
  /** 当前 scene 的路径段（根 scene 为空数组） */
  @Prop scenePath: StarMapPathSegment[] = []
  /** 当前 scene 对应的星图 ID */
  @Prop starmapId: string = ''
  /** 当前 scene 的深度（根为 0） */
  @Prop sceneDepth: number = 0
  /** 主题配置 */
  @Prop themeConfig: ThemeConfig = {
    mode: 'light',
    colorSource: 'built_in',
    selectedPaletteId: '',
    selectedBuiltinThemeId: '',
    colors: {
      primary: '#000000', onPrimary: '#FFFFFF',
      primaryContainer: '#E0E0E0', onPrimaryContainer: '#1A1C1E',
      secondary: '#51606F', onSecondary: '#FFFFFF',
      secondaryContainer: '#D5E2F0', onSecondaryContainer: '#0D1E2C',
      tertiary: '#695779', onTertiary: '#FFFFFF',
      tertiaryContainer: '#F0DBFF', onTertiaryContainer: '#241533',
      error: '#BA1A1A', onError: '#FFFFFF',
      errorContainer: '#FFDAD6', onErrorContainer: '#410002',
      background: '#FDFCFF', onBackground: '#1A1C1E',
      surface: '#FDFCFF', onSurface: '#1A1C1E',
      surfaceVariant: '#DFE2EB', onSurfaceVariant: '#43474E',
      surfaceContainerLowest: '#FFFFFF', surfaceContainerLow: '#F7F8FA',
      surfaceContainer: '#F1F2F5', surfaceContainerHigh: '#EBEDF0',
      surfaceContainerHighest: '#E6E7EA', surfaceDim: '#DAD9DD',
      surfaceBright: '#FDFCFF',
      outline: '#73777F', outlineVariant: '#C3C7CF',
      inverseSurface: '#2F3033', inverseOnSurface: '#F1F0F4',
      inversePrimary: '#A8C8FF',
      scrim: '#000000',
      textPrimary: '#1A1C1E', textSecondary: '#43474E',
      textHint: '#73777F', success: '#2ECC71',
      divider: '#E0E0E0', overlay: '#80000000'
    },
    fontSize: 14,
    fontFamily: 'HarmonyOS Sans',
    lineHeight: 1.5
  }
  /** 运动配置 */
  @Prop motionConfig: PlatformMotionConfig = {
    enabled: true, reduceMotion: false,
    idleWobbleEnabled: true, idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200, dragLiftScale: 1.04,
    settleDurationMs: 220
  }
  /** 动画进度 */
  @Prop @Watch('onMotionProgressChange') motionProgress: number = 0

  // ─── @Link：与父组件双向绑定 ───
  /** 当前星图的图数据 */
  @Link @Watch('onGraphChange') graph: StarMapGraph | null
  // ─── @Prop：全局视角相机（#818）───
  // 整棵递归星图只有一台相机，状态由 StarMapScreen 持有。
  // 子 Scene 也拿到同一份值：它们只是相机里的内容，不能有自己独立的用户缩放。
  /** 全局相机比例 */
  @Prop @Watch('onTransformChange') cameraScale: number = 1
  /** 全局相机 X 偏移（根 Scene 局部屏幕坐标） */
  @Prop @Watch('onTransformChange') cameraOffsetX: number = 0
  /** 全局相机 Y 偏移（根 Scene 局部屏幕坐标） */
  @Prop @Watch('onTransformChange') cameraOffsetY: number = 0

  /**
   * 改全局相机的回调。只有根 Scene 会调，一路传到 StarMapScreen 写回相机。
   * 子 Scene 没有可改的缩放，所以它们只是把 cameraScale/Offset 原样往下传。
   */
  onCameraChanged: (scale: number, offsetX: number, offsetY: number) => void =
    (_scale: number, _offsetX: number, _offsetY: number) => {}

  /**
   * 全树唯一选中态。根 Scene 持有唯一一份，沿组件树传到每个子 Scene。
   * @ObjectLink 而不是 @State：它是共享对象，@State 会在每层各存一份拷贝。
   */
  @ObjectLink selectionState: StarMapSelectionState

  // ─── 普通属性：共享对象和回调 ───
  /** Core bridge（用于 mutation） */
  bridge: IWriterCoreBridge | null = null
  /** 手势状态追踪器（共享） */
  gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  /**
   * 递归 Scene 注册表（共享）。统一手势入口靠它从根往下找 Scene 句柄。
   * 没有注册表（比如单测或脱离 StarMapScreen 复用）时退化成"只有本层"。
   */
  sceneRegistry: StarMapSceneRegistry | null = null
  /** 新建节点回调 */
  onCreateNode: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建子星图回调 */
  onCreateEmbed: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建边回调 */
  onCreateEdge: (scenePath: StarMapPathSegment[], starmapId: string, fromItemId: string, toItemId: string) => void = (_path, _id, _from, _to) => {}
  /** 移动节点回调 */
  onMoveNode: (scenePath: StarMapPathSegment[], starmapId: string, nodeId: string, position: StarMapPoint) => void = (_path, _id, _nodeId, _pos) => {}
  /** 移动 Embed 回调 */
  onMoveEmbed: (scenePath: StarMapPathSegment[], starmapId: string, embedInstanceId: string, position: StarMapPoint) => void = (_path, _id, _embedId, _pos) => {}

  // ─── @State：场景局部状态 ───
  /**
   * 局部适配比例：由本 Scene 的内容包围盒 + 父 Embed 可用区域算出来。
   * 这是**布局适配**，不是相机：用户改不了它，只有内容或容器尺寸变化会重算。
   * 根 Scene 不用它（根用的是全局相机）。
   */
  @State fitScale: number = 1
  /** 局部适配的居中偏移 X（子 Scene 用） */
  @State fitOffsetX: number = 0
  /** 局部适配的居中偏移 Y（子 Scene 用） */
  @State fitOffsetY: number = 0
  /** 平台层布局节点（普通节点） */
  @State platformLayoutNodes: PlatformLayoutNode[] = []
  /** 平台层布局节点（Embed） */
  @State embedLayoutNodes: PlatformLayoutNode[] = []
  /** 节点显示矩形列表（含 Embed） */
  @State nodeRects: PlatformNodeRect[] = []
  /** 边渲染几何列表 */
  @State localEdgeRenders: PlatformEdgeRender[] = []
  /** 当前手势模式 */
  @State gestureMode: StarMapGestureMode = 'idle'
  /** 是否显示节点菜单 */
  @State showNodeMenu: boolean = false
  /** 节点菜单对应的 nodeId */
  @State nodeMenuNodeId: string = ''
  /** 节点菜单锚点（Scene 局部屏幕坐标）：长按时手指所在处，菜单从这儿弹出 */
  @State nodeMenuAnchor: PlatformPoint = { x: 0, y: 0 }
  /** 预览连线起点 */
  @State previewLineFrom: PlatformPoint = { x: 0, y: 0 }
  /** 预览连线终点 */
  @State previewLineTo: PlatformPoint = { x: 0, y: 0 }
  /** 是否显示新建菜单 */
  @State showCreateMenu: boolean = false
  /** 新建菜单锚点（Scene 局部屏幕坐标）：空白长按时手指所在处，菜单从这儿弹出 */
  @State createMenuAnchor: PlatformPoint = { x: 0, y: 0 }
  /** 新建菜单位置（画布坐标）：新建项落在哪张星图的哪个位置 */
  @State createMenuCanvasPosition: PlatformPoint = { x: 0, y: 0 }
  /** Canvas 是否就绪 */
  @State canvasReady: boolean = false
  /** 预览 Canvas 是否就绪 */
  @State previewCanvasReady: boolean = false
  /** Canvas 是否激活 */
  @State canvasActive: boolean = false

  // ─── 私有状态（非响应式） ───
  private motionPauseTracker: MotionPauseTracker = new MotionPauseTracker()
  private panBaseX: number = 0
  private panBaseY: number = 0
  private moveNodeStartX: number = 0
  private moveNodeStartY: number = 0
  private moveEmbedStartX: number = 0
  private moveEmbedStartY: number = 0
  private panActive: boolean = false
  /** 本 Scene 自己的区域尺寸（vp），用于算子图适配比例 */
  private sceneWidth: number = 0
  private sceneHeight: number = 0
  /** 本 Scene 在屏幕上的原点，用来把 screenX / screenY 换算成 Scene 局部坐标。 */
  private sceneGlobalX: number = 0
  private sceneGlobalY: number = 0
  /** 触摸点四种坐标全缺时只打一次日志，避免刷屏。 */
  private hasLoggedBadTouch: boolean = false
  /** 是否已经算过一次 fit view。子图加节点/移动节点不再重算，只有首次加载和显式 fit view 才算（#816） */
  private hasFittedView: boolean = false
  // 上次适配时 Scene 的宽度，用来判断容器尺寸变化是不是父层缩放带来的
  private lastFittedSceneSize: number = 0
  /** 注册到 sceneRegistry 的本层句柄 */
  private sceneHandle: StarMapSceneNode | null = null

  // ─── 全局相机双指缩放状态（只在根 Scene 上被改） ───
  /** 两指抬起到只剩 0 指 */
  private pinchWasActive: boolean = false
  /** 双指落下那一刻的全局相机比例（每一帧都从它算，不吃上一帧的结果） */
  private pinchBaseScale: number = 1
  /** 双指落下那一刻的全局相机偏移 */
  private pinchBaseOffsetX: number = 0
  private pinchBaseOffsetY: number = 0
  /** 本次双指缩放的起始两指距离（捏合幅度的分母，0 表示这帧没有两指） */
  private pinchBaseDistance: number = 0
  /** 当前按下的手指：fingerId → Scene 局部屏幕坐标 */
  private fingerMap: Map<number, PlatformPoint> = new Map<number, PlatformPoint>()
  /** 当前按下的手指坐标快照（顺序不保证，取前两个即可） */
  private fingerPoints: PlatformPoint[] = []

  // ─── 跨层连线状态 ───
  /** connect 起点：相对根星图的完整 StarMapTargetPath（#816 跨层连线靠它） */
  private connectSourcePath: StarMapTargetPath | null = null
  /** connect 起点在本 Scene 的对象 ID（日志用） */
  private connectSourceId: string = ''
  /** connect 起点类型 */
  private connectSourceKind: string = 'node'

  // ─── 对话框宿主（窗口层级，不参与 Scene Stack 布局） ───
  /** 新建节点对话框的 ComponentContent：openCustomDialog 需要它来关闭对话框 */
  private createNodeDialogContent: ComponentContent<CreateTitleDialogArgs> | null = null
  /** 新建子星图对话框的 ComponentContent */
  private createEmbedDialogContent: ComponentContent<CreateTitleDialogArgs> | null = null

  // ─── Canvas 上下文 ───
  private canvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private canvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.canvasSettings)
  private previewCanvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private previewCanvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.previewCanvasSettings)

  // ─── 生命周期 ───

  aboutToAppear() {
    this.canvasActive = true
    this.recomputeLayoutAndGeometry()
    this.registerSceneHandle()
    this.maybeFitView()
  }

  aboutToDisappear() {
    this.canvasActive = false
    this.unregisterSceneHandle()
    // 树里所有 Scene 共用一个 tracker。根 Scene 整页退出可以无条件清空；
    // 子 Scene 退出（Embed 被删、childGraph 重载、loading/error 分支切换）时，
    // 只有当前手势确实属于自己这棵子树才允许清，否则会把兄弟子树或根层
    // 正在进行的手势一起清掉（#816）。
    if (this.sceneDepth === 0 || this.gestureTracker.isOwnedBySceneSubtree(this.scenePath)) {
      this.gestureTracker.reset()
    }
    this.panActive = false
    this.gestureMode = 'idle'
    this.showNodeMenu = false
    this.showCreateMenu = false
    this.nodeMenuNodeId = ''
    this.closeCreateDialogs()
  }

  /**
   * 本 Scene 的有效比例：根是全局相机比例，子是局部适配比例（#818）。
   * 渲染、命中、边几何全部用这个值，界面上不允许再直接读 cameraScale / fitScale 当比例用。
   *
   * 必须是**方法**，不能是 getter：ArkUI 状态管理 V1 的 @Component 不支持 get
   * 访问器，编译器会把它当成一个从没被赋值的普通属性，于是每次读都是 undefined。
   * #816 首轮真机复测就是这么塌的 —— viewportScale 恒为 undefined，往下传出去：
   *   - syncSceneHandle() 把 undefined 塞进 StarMapSceneNode.scale，递归命中
   *     读到的比例全是 undefined，screenToCanvas 出来是 NaN，命中全部落空成 blank；
   *   - computeCameraPinch 拿到 NaN 比例，cameraOffsetX / cameraOffsetY 变成 NaN；
   *   - 空白处长按换算出来的 createMenuCanvasPosition 是 NaN，序列化成 null，
   *     Core 解析节点报 PARSE_ERROR，新建节点 / 子星图一律失败。
   * 只有 @ComponentV2 才支持 getter（HomeScreen.ets 那种写法），这里是 V1，别改回去。
   */
  private viewportScaleValue(): number {
    return this.isCameraScene() ? this.cameraScale : this.fitScale
  }

  /** 本 Scene 的视口偏移 X：根用全局相机，子用局部适配。 */
  private viewportOffsetX(): number {
    return this.isCameraScene() ? this.cameraOffsetX : this.fitOffsetX
  }

  /** 本 Scene 的视口偏移 Y：根用全局相机，子用局部适配。 */
  private viewportOffsetY(): number {
    return this.isCameraScene() ? this.cameraOffsetY : this.fitOffsetY
  }

  /** 本 Scene 是不是相机本体（全树只有根 Scene 是）。 */
  private isCameraScene(): boolean {
    return this.sceneDepth === 0
  }

  /**
   * 本 Scene 现在能不能改视口。
   *
   * 双指进行中时谁都别动视口：相机只有一台，双指正在改它的时候再叠一次单指平移，
   * 屏幕上就是"缩放抖一下"。
   */
  private canMutateViewport(): boolean {
    return !this.gestureTracker.isPinching()
  }

  // ─── 递归 Scene 句柄与注册表 ───

  /**
   * 构造本 Scene 在注册表里的记录。
   *
   * 记录上的几何字段是**快照**：递归命中要读 rects / scale / offset，
   * 所以每次重算几何都重新 sync 一次，否则统一手势入口拿到的是上一次布局的旧数据，
   * 会把手指判到别的对象上。
   */
  private buildSceneHandle(): StarMapSceneNode {
    if (this.sceneHandle === null) {
      this.sceneHandle = new StarMapSceneNode(
        this.scenePath,
        this.starmapId,
        (): RecursiveSceneSource[] => this.collectChildEmbeds(),
        (from: StarMapTargetPath, to: StarMapTargetPath): Promise<boolean> => {
          return this.createEdgeBetween(from, to)
        },
        (): void => {
          this.drawEdges()
        }
      )
    }
    this.sceneHandle.sync(
      this.nodeRects, this.getEmbedInstanceIds(), this.localEdgeRenders,
      this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
    return this.sceneHandle
  }

  private registerSceneHandle(): void {
    if (this.sceneRegistry === null) {
      return
    }
    this.sceneRegistry.register(this.buildSceneHandle())
  }

  private unregisterSceneHandle(): void {
    if (this.sceneRegistry !== null) {
      this.sceneRegistry.unregister(this.scenePath)
    }
    this.sceneHandle = null
  }

  /**
   * 本层所有**已加载完成**的子 Scene 句柄。
   * 子图还没加载完就没有对应项，此时递归命中会停在"点在这个 Embed 上"，
   * 行为等价于"点子星图标题选中它"，不会退化成选空白。
   */
  private collectChildEmbeds(): RecursiveSceneSource[] {
    const out: RecursiveSceneSource[] = []
    if (this.sceneRegistry === null || !this.graph) {
      return out
    }
    for (const embed of this.graph.embeds) {
      const child: StarMapSceneNode | null =
        this.sceneRegistry.findChildEmbed(this.scenePath, embed.instanceId)
      if (child !== null) {
        out.push(child)
      }
    }
    return out
  }

  /**
   * 取整棵 Scene 树的递归上下文。统一手势入口用它做命中和坐标换算。
   * 没有注册表（脱离 StarMapScreen 单独复用）时返回 null，调用方退回本层行为。
   */
  private getRootContext(): RecursiveSceneContext | null {
    if (this.sceneRegistry === null) {
      return null
    }
    const root: StarMapSceneNode | null = this.sceneRegistry.getRoot()
    if (root === null) {
      return null
    }
    return buildRecursiveSceneContext(root)
  }

  // ─── 视口适配（fit view） ───

  /**
   * 首次布局完成时算一次 fit view。
   *
   * 只在"还没有算过"时算：子图后来加节点、移动节点都不再重算，
   * 否则用户刚双指放大好的视图会被一次内容变化打回初始大小（#816 明确要求）。
   * 根 Scene 恒不自动适配（整屏显示，行为与之前一致）。
   */
  private maybeFitView(): void {
    if (this.hasFittedView) {
      return
    }
    // 根 Scene 不自动适配：它本来就整屏显示，工具栏 +/- 直接作用在它身上，
    // 自动适配会改掉这层一直以来的初始视图。
    if (this.sceneDepth === 0) {
      return
    }
    if (this.sceneWidth <= 0 || this.sceneHeight <= 0 || !this.graph) {
      return
    }
    this.fitView()
  }

  /**
   * 按当前内容重新适配视口（首次加载 / 显式 fit view）。
   *
   * 可用区域取父 Embed 圆形内容区里"手能可靠点到"的内接正方形边长：
   * 子 Scene 的布局是正方形，四角会超出圆被父层 clip 裁掉，
   * 所以按 1/√2 折算，内容不会铺到圆外被切掉。
   */
  fitView(): void {
    this.hasFittedView = true
    const bounds: ContentBounds | null = computeContentBounds(this.nodeRects)
    if (bounds === null) {
      this.fitScale = 1
      this.fitOffsetX = 0
      this.fitOffsetY = 0
      this.lastFittedSceneSize = this.sceneWidth
      this.syncSceneHandle()
      return
    }
    const available: number = Math.min(this.sceneWidth, this.sceneHeight) * CIRCLE_INNER_SAFE_RATIO
    // 比例按可用区（圆的内接正方形）算，偏移按 Scene 自己的尺寸算：
    // 子 Scene 组件是铺满整个圆的，中心在 sceneWidth/2、sceneHeight/2。
    // 拿内接正方形的左上角当原点会让内容整体偏左上（#816 复审第 4 条）。
    const fitted: Viewport = computeFittedViewport(
      bounds, available, available, EMBED_FIT_PADDING_VP, this.sceneWidth, this.sceneHeight
    )
    this.fitScale = fitted.zoomScale
    this.fitOffsetX = fitted.offsetX
    this.fitOffsetY = fitted.offsetY
    this.lastFittedSceneSize = this.sceneWidth
    // fitScale 变了，递归命中/坐标换算读的就是这份快照，必须立刻同步
    this.syncSceneHandle()
  }

  /**
   * Embed 容器尺寸变化时重新居中（#818）。
   *
   * 容器变大现在只有一个原因：全局相机缩放了，父 Scene 的比例跟着变，
   * 圆壳和铺满圆壳的子 Scene 组件一起变大。
   *
   * 注意这里**不再同比放大 fitScale**：容器在屏幕上已经跟着相机放大了，
   * 子内容用的还是同一个 fitScale，屏幕上的呈现就是同比放大——这正是我们要的。
   * 如果再乘一次尺寸比，等于把相机的缩放抵消掉，用户双指放大整棵树时
   * 就会出现"外层圆变大、里面内容纹丝不动"。
   *
   * 只重算居中偏移：fitScale 保持不变，偏移按新容器尺寸重新对一次中心。
   */
  private syncFitToSceneSize(): void {
    if (!this.hasFittedView || this.lastFittedSceneSize <= 0) {
      return
    }
    if (Math.abs(this.sceneWidth - this.lastFittedSceneSize) <= 0.5) {
      return
    }
    this.lastFittedSceneSize = this.sceneWidth
    const bounds: ContentBounds | null = computeContentBounds(this.nodeRects)
    if (bounds === null) {
      this.fitOffsetX = 0
      this.fitOffsetY = 0
    } else {
      const offset: PlatformPoint = computeCenteredOffset(
        bounds, this.fitScale, this.sceneWidth, this.sceneHeight
      )
      this.fitOffsetX = offset.x
      this.fitOffsetY = offset.y
    }
    this.syncSceneHandle()
  }

  /**
   * 把当前视口和矩形快照写回 Scene 句柄。
   *
   * 递归命中、坐标换算全都从注册表里读 Scene 的 scale/offset，
   * 而相机/局部适配改的是 @Prop 上的 cameraScale、@State 上的 fitScale/fitOffset，
   * 两者之间没有自动联动。这里是唯一把它们对齐的地方。
   */
  private syncSceneHandle(): void {
    if (this.sceneHandle === null) {
      return
    }
    this.sceneHandle.sync(
      this.nodeRects, this.getEmbedInstanceIds(), this.localEdgeRenders,
      this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
  }

  /** 改全局相机。只有根 Scene 能调，子 Scene 没有独立相机可改（#818）。 */
  private applyCameraTransform(scale: number, offsetX: number, offsetY: number): void {
    if (!this.isCameraScene()) {
      return
    }
    this.onCameraChanged(scale, offsetX, offsetY)
  }

  /**
   * 写本 Scene 的视口偏移。
   * 根 Scene 改全局相机；子 Scene 只能改自己的局部适配偏移（用户在子图里平移内容）。
   */
  private setViewportOffset(offsetX: number, offsetY: number): void {
    if (this.isCameraScene()) {
      this.applyCameraTransform(this.cameraScale, offsetX, offsetY)
      return
    }
    this.fitOffsetX = offsetX
    this.fitOffsetY = offsetY
  }

  // ─── 统一双指缩放入口 = 全局相机（只在根 Scene 上生效，#818） ───

  /**
   * 根 Scene 统一手势入口：原始触摸事件。
   *
   * 用 onTouch 而不是 PinchGesture 的原因：
   * 1. PinchGesture 要参加手势竞争，落在子星图上的那根手指会被子 Scene 先吃掉；
   *    而 onTouch 是**冒泡**事件，祖先节点一定收得到子节点的触摸。
   * 2. 挂在最底下那个空白 Column 上只覆盖"手指落在空白"的情况，空白 Column 只是
   *    Stack 里的一个兄弟节点，手指一旦落在子星图上就绕开它了（#816 真机症状）。
   *
   * 两指缩放整棵树的缩放锚点、是否允许改视口，全部由这里统一决定。
   * 这里**不再**查递归命中：缩放目标只有全局相机一个，不存在"缩放哪一层"的问题（#818）。
   */
  private onSceneTouch(event: TouchEvent): void {
    // 只有根 Scene 是统一入口。子 Scene 不重复起手势，否则相机每帧被改两次。
    if (!this.isCameraScene()) {
      return
    }
    this.trackFinger(event)
    if (this.fingerPoints.length >= 2) {
      const center: PlatformPoint = twoFingerCenter(this.fingerPoints[0], this.fingerPoints[1])
      if (!this.pinchWasActive) {
        this.beginUnifiedPinch(center)
      }
      if (this.pinchWasActive) {
        this.updateUnifiedPinch(center)
      }
    } else if (this.pinchWasActive) {
      this.endUnifiedPinch()
    }
  }

  /**
   * 用 changedTouches 维护当前按下的手指集合。
   * 不用 event.touches 是因为抬手那一帧 touches 的组成各家实现不一致，
   * 而 changedTouches 一定只含"这一帧发生变化"的手指。
   */
  private trackFinger(event: TouchEvent): void {
    const changed: TouchObject[] = event.changedTouches !== undefined && event.changedTouches !== null
      ? event.changedTouches
      : []
    for (const touch of changed) {
      if (event.type === TouchType.Up || event.type === TouchType.Cancel) {
        this.fingerMap.delete(touch.id)
      } else {
        this.fingerMap.set(touch.id, this.resolveTouchLocalPoint(touch))
      }
    }
    this.fingerPoints = []
    this.fingerMap.forEach((point: PlatformPoint) => {
      this.fingerPoints.push(point)
    })
  }

  /**
   * 把一个触摸点换算成本 Scene 的局部坐标。
   *
   * 真机上不能只认 \`touch.x / touch.y\`：#816 首轮复测在"双指一缩放就闪退"，
   * 异常栈是 \`logPinchBegin\` 里对可能为 undefined 的坐标调 \`.toFixed()\`。
   * 相对坐标字段并不总是有值——ArkUI 只保证 \`screenX / screenY\` 是屏幕绝对坐标。
   *
   * 这里优先用相对坐标（它是 Scene 组件内布局真正用的口径），只有在它不是有限数
   * 时才退回绝对坐标减掉本 Scene 在屏幕上的原点。两条路都拿不到就记 0，并留一条
   * error 日志把触摸点的原始字段全打出来——下次再出问题可以直接从日志定位，
   * 不用再靠猜。
   */
  private resolveTouchLocalPoint(touch: TouchObject): PlatformPoint {
    const relativeX: number = touch.x
    const relativeY: number = touch.y
    if (Number.isFinite(relativeX) && Number.isFinite(relativeY)) {
      return { x: relativeX, y: relativeY }
    }
    const screenX: number = touch.screenX
    const screenY: number = touch.screenY
    if (Number.isFinite(screenX) && Number.isFinite(screenY)) {
      return { x: screenX - this.sceneGlobalX, y: screenY - this.sceneGlobalY }
    }
    if (!this.hasLoggedBadTouch) {
      this.hasLoggedBadTouch = true
      hilog.error(
        0x0000, 'StarMapGesture',
        `#816 touch_without_any_coordinate id=${touch.id} touchType=${touch.type} ` +
        `x=${String(relativeX)} y=${String(relativeY)} ` +
        `screenX=${String(screenX)} screenY=${String(screenY)} ` +
        `sceneGlobalX=${this.sceneGlobalX} sceneGlobalY=${this.sceneGlobalY}`
      )
    }
    return { x: 0, y: 0 }
  }

  /** 两指之间的屏幕距离（捏合幅度的度量）。 */
  private fingerDistance(a: PlatformPoint, b: PlatformPoint): number {
    const dx: number = a.x - b.x
    const dy: number = a.y - b.y
    return Math.sqrt(dx * dx + dy * dy)
  }

  /**
   * 两指落下：记下全局相机的起点和捏合幅度的起点（#818）。
   *
   * 这里**不查递归命中**：两指落在节点上、子星图上、二层子星图上，结果完全一样，
   * 缩的都是整棵树。只有"起手相机比例、起手两指距离、起手两指中心对应的 world 坐标"
   * 三件事要记住，每一帧都从这三点重算，而不是从上一帧的结果往上叠。
   */
  private beginUnifiedPinch(rootCenter: PlatformPoint): void {
    this.pinchWasActive = true
    this.pinchBaseScale = this.cameraScale > 0 ? this.cameraScale : 1
    this.pinchBaseOffsetX = this.cameraOffsetX
    this.pinchBaseOffsetY = this.cameraOffsetY
    this.pinchBaseDistance = this.fingerPoints.length >= 2
      ? this.fingerDistance(this.fingerPoints[0], this.fingerPoints[1])
      : 0
    this.gestureTracker.beginPinch(rootCenter.x, rootCenter.y)
    logViewportState(this.pinchBaseScale, this.pinchBaseOffsetX, this.pinchBaseOffsetY)
    logPinchBegin(rootCenter.x, rootCenter.y, this.pinchBaseScale)
  }

  /**
   * 两指移动：改全局相机。
   *
   * 比例来自"当前两指距离 ÷ 起手两指距离"，永远是相对起手那一帧算的，
   * 所以每帧都传 `pinchBaseScale` 而不是 `cameraScale`——后者每帧都被我们自己改过，
   * 用它当基准会让比值在帧间自乘（两指 100 → 110 → 120 变成 1.32 而不是 1.20）。
   */
  private updateUnifiedPinch(rootCenter: PlatformPoint): void {
    let ratio: number = 1
    if (this.pinchBaseDistance > 0 && this.fingerPoints.length >= 2) {
      ratio = this.fingerDistance(this.fingerPoints[0], this.fingerPoints[1]) / this.pinchBaseDistance
    }
    if (!isFinite(ratio) || ratio <= 0) {
      return
    }
    const next: GlobalCameraTransform = computeCameraPinch(
      {
        scale: this.pinchBaseScale,
        offsetX: this.pinchBaseOffsetX,
        offsetY: this.pinchBaseOffsetY
      },
      this.pinchBaseScale,
      this.pinchBaseDistance,
      this.fingerDistance(this.fingerPoints[0], this.fingerPoints[1]),
      rootCenter.x,
      rootCenter.y
    )
    this.applyCameraTransform(next.scale, next.offsetX, next.offsetY)
  }

  /** 双指抬起：清掉手势状态，避免下次沿用上一次的比例基准。 */
  private endUnifiedPinch(): void {
    if (!this.pinchWasActive) {
      return
    }
    logPinchEnd(this.cameraScale)
    this.pinchWasActive = false
    this.gestureTracker.endPinch()
  }

  /** 按绝对路径查 Scene 句柄；查不到退回本层句柄。 */
  private findSceneHandle(scenePath: StarMapPathSegment[]): StarMapSceneNode | null {
    if (this.sceneRegistry === null) {
      return this.buildSceneHandle()
    }
    const found: StarMapSceneNode | null = this.sceneRegistry.find(scenePath)
    return found !== null ? found : this.buildSceneHandle()
  }

  // ─── 选中态 ───

  private isNodeSelected(nodeId: string): boolean {
    return this.selectionState.isSelected(this.scenePath, 'node', nodeId)
  }

  private isEmbedSelected(instanceId: string): boolean {
    return this.selectionState.isSelected(this.scenePath, 'embed', instanceId)
  }

  /**
   * 选中一个对象。
   *
   * 全树只有一份状态，且身份是 scenePath + kind + itemId：
   * 选子图里的节点会立刻让父层那个 Embed 取消选中（因为 scenePath 不同），
   * 选二级子星图的标题也只会选中那一个 Embed。
   */
  private selectItem(kind: 'node' | 'embed' | 'edge', itemId: string): void {
    logSelectionChanged(describeScenePath(this.scenePath), kind, itemId)
    this.selectionState.select(this.scenePath, kind, itemId)
    this.redrawAllSceneEdges()
  }

  /**
   * 共享选中一变就让**每一层**重画边。
   *
   * 边的高亮是 drawEdges 里读 selectionState 画的，而 ArkUI 的 Canvas onReady 只在
   * 首次准备好时触发一次，外部状态变化之后不会自动重放之前的绘图命令。原来能触发
   * 重画的只有视口变化、动画帧、图更新和首次 onReady —— 选中边以后如果动画是关的
   * （reduce motion / shouldAnimate 为 false），线可以一直停在旧颜色旧线宽，直到下一次
   * 缩放或平移才被改对。
   * 更要紧的是选中是整棵树共享的一份：先选子层的边再点根层的节点，子层那条高亮也得
   * 擦掉，所以只重画当前这层不够，必须遍历注册表里所有已注册的 Scene。
   */
  private redrawAllSceneEdges(): void {
    if (this.sceneRegistry === null) {
      // 没有注册表（独立使用本组件时）只能重画自己
      this.drawEdges()
      return
    }
    this.sceneRegistry.redrawAllEdges()
  }

  /**
   * 空白层单击：选中递归命中的目标，没有目标就清掉选中。
   *
   * 这一条同时补上了 #816 两个一直没闭环的地方：
   * - 边以前有选中状态、有选中画法，但**没有任何输入入口**能把它设成选中；
   * - clearSelection() 以前定义了但全项目没有调用点，点空白不会清选中。
   *
   * 两者共用同一个递归命中：点的位置换算到根坐标，resolveRecursiveHit 返回什么
   * 就选什么（含子层里的边，scenePath 自然是子层路径），返回 null 就清掉。
   * 节点 / Embed 自己的单击不走这里——它们各自组件在 Stack 上层，事件被它们收走，
   * 根本到不了背景层。
   */
  private handleBlankTap(screenX: number, screenY: number) {
    const context: RecursiveSceneContext | null = this.getRootContext()
    if (context === null) {
      return
    }
    const rootPoint: PlatformPoint = convertPointToRoot(context, this.scenePath, screenX, screenY)
    const hit: RecursiveHit = resolveRecursiveHit(context, rootPoint.x, rootPoint.y)
    if (hit.target === null) {
      this.clearSelection()
      return
    }
    this.selectionState.select(hit.target.scenePath, hit.target.objectKind, hit.target.objectId)
    logSelectionChanged(
      describeScenePath(hit.target.scenePath), hit.target.objectKind, hit.target.objectId
    )
    this.redrawAllSceneEdges()
  }

  /** 点空白：清空全树选中。 */
  private clearSelection(): void {
    if (!this.selectionState.hasSelection()) {
      return
    }
    logSelectionChanged(describeScenePath(this.scenePath), 'none', '')
    this.selectionState.clear()
    this.redrawAllSceneEdges()
  }

  // ─── @Watch 处理 ───

  onTransformChange() {
    // 相机变了（#818）：递归命中和坐标换算读的是注册表里的快照，
    // 不同步的话缩放之后命中判定就一直用的是旧视口（#816 复审第 2 条）。
    this.syncSceneHandle()
    this.drawEdges()
  }

  onMotionProgressChange() {
    this.drawEdges()
    if (this.gestureMode === 'connect') {
      this.drawPreviewLine()
    }
  }

  onGraphChange() {
    this.recomputeLayoutAndGeometry()
    // 图第一次到位时布局区可能已经有尺寸了，onAreaChange 不一定还会再来一次
    this.maybeFitView()
  }

  // ─── 布局与几何计算 ───

  /**
   * 从 graph 重建布局节点和几何数据。
   * 在 graph 变化、节点/Embed 移动后调用。
   */
  recomputeLayoutAndGeometry() {
    if (!this.graph) {
      this.platformLayoutNodes = []
      this.embedLayoutNodes = []
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
    this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds)
    this.recomputeGeometry()
  }

  /**
   * 重新计算几何：nodeRects + localEdgeRenders。
   * 在节点位置变化、graph 加载后调用。
   */
  recomputeGeometry() {
    if (!this.graph) {
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    const nodeRects: PlatformNodeRect[] = this.platformLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    const embedRects: PlatformNodeRect[] = this.embedLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    this.nodeRects = nodeRects.concat(embedRects)
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of this.nodeRects) {
      rectMap.set(rect.nodeId, rect)
    }
    this.localEdgeRenders = computeAllEdgeRenders(this.graph.edges, rectMap, this.starmapId, this.getEmbedInstanceIds())
    // 布局变了就同步快照：递归命中要读的是最新 rects
    this.buildSceneHandle()
    this.drawEdges()
  }

  // ─── 坐标计算 ───

  /**
   * 获取平台层布局节点（普通节点或 Embed）
   */
  private getPlatformLayoutNode(nodeId: string): PlatformLayoutNode | undefined {
    const node = this.platformLayoutNodes.find(ln => ln.nodeId === nodeId)
    return node !== undefined ? node : this.embedLayoutNodes.find(ln => ln.nodeId === nodeId)
  }

  /**
   * 计算节点在屏幕上的最终 x 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenX(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseX = ln ? ln.x : 0
    const wobbleDx = wobbleXFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseX + wobbleDx) * this.viewportScaleValue() + this.viewportOffsetX()
  }

  /**
   * 计算节点在屏幕上的最终 y 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenY(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseY = ln ? ln.y : 0
    const wobbleDy = wobbleYFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseY + wobbleDy) * this.viewportScaleValue() + this.viewportOffsetY()
  }

  /**
   * 把预览连线的起点更新到"源对象边界上朝向手指的那个点"。
   *
   * 必须复用 geometry 的 edgeEndpointBoundaryPoint（正式边渲染也走它）：
   * 早期版本这里直接取对象中心当起点，导致拉线预览从黑圆正中心出发，
   * 松手后正式边又落在圆周，中间跳一下（Issue #813 复审）。
   * 普通节点和 Embed 统一走这个入口，两种形状的边界算法由 geometry 决定。
   *
   * @param sourceId 连线源对象 ID（普通节点或 Embed）
   * @param pointerScreen 当前手指位置的屏幕坐标
   */
  private updatePreviewLineFrom(sourceId: string, pointerScreen: PlatformPoint) {
    const ln = this.getPlatformLayoutNode(sourceId)
    if (!ln) return
    const rect: PlatformNodeRect = deriveNodeRectFromLayoutNode(ln)
    const pointerCanvas: PlatformPoint = screenToCanvas(
      pointerScreen.x, pointerScreen.y, this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY()
    )
    const start: PlatformPoint = edgeEndpointBoundaryPoint(
      rect, this.isEmbed(sourceId), pointerCanvas.x, pointerCanvas.y
    )
    this.previewLineFrom = {
      x: start.x * this.viewportScaleValue() + this.viewportOffsetX(),
      y: start.y * this.viewportScaleValue() + this.viewportOffsetY()
    }
  }

  /**
   * 计算 Embed 在屏幕上的 x 坐标（无 wobble）
   */
  private getEmbedScreenX(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseX = ln ? ln.x : 0
    return baseX * this.viewportScaleValue() + this.viewportOffsetX()
  }

  /**
   * 计算 Embed 在屏幕上的 y 坐标
   */
  private getEmbedScreenY(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseY = ln ? ln.y : 0
    return baseY * this.viewportScaleValue() + this.viewportOffsetY()
  }

  /**
   * 判断 nodeId 是否是 Embed
   */
  private isEmbed(nodeId: string): boolean {
    if (!this.graph) return false
    return this.graph.embeds.some(e => e.instanceId === nodeId)
  }

  // ─── 手势辅助 ───

  /**
   * 手势触点的屏幕坐标。
   */
  private gestureScreenPoint(
    event: GestureEvent | undefined,
    anchorX: number,
    anchorY: number
  ): PlatformPoint {
    if (!event || !event.fingerList || event.fingerList.length === 0) {
      return { x: anchorX, y: anchorY }
    }
    const finger = event.fingerList[0]
    if (!finger) {
      return { x: anchorX, y: anchorY }
    }
    return {
      x: anchorX + finger.localX,
      y: anchorY + finger.localY
    }
  }

  // ─── Mutation 方法 ───

  /**
   * 构造 item 的 StarMapTargetPath 端点，**统一相对根星图**。
   *
   * 之前这里是以"当前 Scene 的 starmapId + 空 segments"起手，于是每层的端点路径
   * 只在自己那一层里有效，"父节点拖到子节点上"这种跨层连线根本无法表达。
   * 现在起点统一相对根星图：segments 就是本 Scene 的绝对路径（Embed 再追加一段 enterEmbed），
   * 之后 planCrossLayerEdge 负责把它换算到真正的宿主 Scene。
   */
  private buildItemTargetPath(itemId: string): StarMapTargetPath {
    return buildTargetPathForHost(this.rootStarmapId, [], {
      scenePath: this.scenePath,
      starmapId: this.starmapId,
      kind: this.isEmbed(itemId) ? 'embed' : 'node',
      itemId: itemId
    })
  }

  // ─── 跨层连线 ───

  /**
   * 开始连线：记下起点相对**根星图**的完整路径。
   *
   * 起点必须存完整路径而不是"本层的 itemId"：跨层连线要靠它和终点的路径
   * 求最近公共祖先，宿主是父层还是祖父层完全由这两条路径决定。
   */
  beginConnectFrom(itemId: string, point: PlatformPoint) {
    const sourcePath: StarMapTargetPath = this.buildItemTargetPath(itemId)
    this.gestureTracker.beginConnect(this.scenePath, itemId, point.x, point.y)
    this.connectSourcePath = sourcePath
    this.connectSourceId = itemId
    this.connectSourceKind = this.isEmbed(itemId) ? 'embed' : 'node'
    this.previewCanvasReady = false
    this.gestureMode = 'connect'
    this.showNodeMenu = false
    this.updatePreviewLineFrom(itemId, point)
    this.previewLineTo = point
    logConnectBegin(sourcePath)
  }

  /**
   * 结束连线：用递归命中解析终点，再按最近公共祖先决定边落在哪张图上。
   *
   * 五种情形（#816 明确要求覆盖）都收敛到同一条规则：
   * 父 Node → 子 Node、子 Node → 父 Node、同一 Embed 里的两个子 Node、
   * 两个不同 Embed 里的 Node、子 Node → 更深一层的子 Embed/Node。
   * 同层连线算出来的方案和之前完全一致，所以不需要第二套分支。
   */
  async finishConnect(localScreenX: number, localScreenY: number) {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    if (sourcePath === null) {
      return
    }
    const context: RecursiveSceneContext | null = this.getRootContext()
    if (context === null) {
      logConnectEnd(sourcePath, null, '', false, 'no_scene_context')
      return
    }

    // 本 Scene 局部坐标 → 根 Scene 局部坐标，递归命中只能在根坐标系里做
    const rootPoint: PlatformPoint = convertPointToRoot(
      context, this.scenePath, localScreenX, localScreenY
    )
    const hit: RecursiveHit = resolveRecursiveHit(context, rootPoint.x, rootPoint.y)
    logRecursiveHit(hit)

    if (hit.target === null) {
      logConnectEnd(sourcePath, null, '', false, 'blank_target')
      return
    }
    if (hit.target.objectKind === 'embed' && hit.target.hitRegion === 'innerContent' &&
      this.isSamePath(hit.target.scenePath, this.scenePath) && hit.target.objectId === this.connectSourceId) {
      logConnectEnd(sourcePath, null, '', false, 'self')
      return
    }
    // 递归命中现在能返回 edge，但边不是合法的连线端点：把 edgeId 当成 nodeId 递给
    // Core 就是拿一条不存在的节点去建边。统一命中可以共用，可命中的对象集合却不能
    // 直接当成连线的合法端点集合（#816 复审第 8 条）。
    if (hit.target.objectKind === 'edge') {
      logConnectEnd(sourcePath, null, '', false, 'edge_target')
      return
    }

    const targetPath: StarMapTargetPath = buildTargetPathForHost(this.rootStarmapId, [], {
      scenePath: hit.target.scenePath,
      starmapId: hit.target.starmapId,
      kind: hit.target.objectKind === 'embed' ? 'embed' : 'node',
      itemId: hit.target.objectId
    })
    const plan = planCrossLayerEdge(context, sourcePath, targetPath)
    if (plan === null) {
      logConnectEnd(sourcePath, targetPath, '', false, 'same_object')
      return
    }

    // 边只能落在宿主 Scene 上：把两条相对宿主的端点路径交给它去写 Core
    const hostHandle: StarMapSceneNode | null = this.findSceneHandle(plan.hostScenePath)
    if (hostHandle === null) {
      logConnectEnd(sourcePath, targetPath, plan.hostStarmapId, false, 'no_host_scene')
      return
    }
    // 必须 await：诊断要能区分"递归命中没找到目标"和"路径算出来了但 Core 拒绝"，
    // 不等结果的话 connect_end 只会在取消路径上被写到（#816 复审第 6 条）。
    const created: boolean = await hostHandle.createEdgeBetween(plan.from, plan.to)
    logConnectEnd(sourcePath, targetPath, plan.hostStarmapId, created, created ? '' : 'core_failed')
  }

  private isSamePath(a: StarMapPathSegment[], b: StarMapPathSegment[]): boolean {
    if (a.length !== b.length) {
      return false
    }
    for (let i = 0; i < a.length; i++) {
      if (a[i].type !== b[i].type || a[i].instanceId !== b[i].instanceId || a[i].nodeId !== b[i].nodeId) {
        return false
      }
    }
    return true
  }

  /**
   * 连线收尾：只有在全局状态**仍然**是我发起的 connect 时才落边并清空 tracker。
   *
   * 第二根手指落下后根 Scene 会把全局状态切成 pinch，这时旧 connect 的
   * onActionEnd / onActionCancel 只准清自己的预览，不准碰全局 tracker，
   * 否则刚认领的双指缩放会被一起清掉（#816）。
   */
  private finishOwnedConnect() {
    const state: GestureState = this.gestureTracker.getState()
    if (state.mode === 'connect' && this.gestureTracker.isConnectOwnedByScene(this.scenePath)) {
      // 终点在根坐标系里做递归命中，父子跨层的边由最近公共祖先那张图承接
      void this.finishConnect(this.previewLineTo.x, this.previewLineTo.y)
      this.gestureTracker.reset()
    } else {
      // 已被 pinch 接管：不动全局，但要把这条 connect 的结束边界补上
      this.closeSupersededConnectIfNeeded()
    }
    this.clearConnectVisualState()
  }

  /**
   * 连线被**新手势接管**（这里只可能是 pinch）时，补齐这条 connect 的结束边界日志。
   *
   * #816 要求 connect_begin / connect_end 成对出现。真机上会出现这样的时序：
   * beginConnect → logConnectBegin → 第二根手指落下 → beginPinch → 旧 connect 的
   * onActionEnd / onActionCancel 晚到。此时 clearIfOwnedBy('connect') 返回 false，
   * 只清预览不打日志，诊断包里就只剩一个没有下文的 connect_begin，下一次看诊断
   * 分不清是"连线卡死"还是"被新手势接管"。
   *
   * 这里刻意和"能不能清新 tracker"分成两件事：
   * - 接管后**绝不能** reset 当前的 pinch，那会把刚认领的缩放一起清掉。
   * - 但"不碰 tracker"不等于"不能给自己这条旧连线写结束日志"。
   *
   * 记完立刻把 connectSourcePath 置空，保证同一次 connect 只记一条；
   * 迟到的 End 和 Cancel 都可能到达，只该有一条。
   */
  private closeSupersededConnectIfNeeded() {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    if (sourcePath === null) {
      return
    }
    if (this.gestureTracker.getState().mode !== 'pinch') {
      return
    }
    this.connectSourcePath = null
    logConnectEnd(sourcePath, null, '', false, 'superseded_by_pinch')
  }

  /**
   * 连线被**取消**（触摸被系统取消，不是抬手）时的收尾。
   *
   * 取消时如果全局仍然是自己发起的 connect，必须把 tracker 清回 idle ——
   * 否则 tracker 会卡在 connect，后续别层手势会因为"不是 idle、owner 也不是我"
   * 被全部挡住。已经被 pinch 接管时 clearIfOwnedBy 返回 false，只清旧预览。
   *
   * 边界日志：清掉 connectSourcePath 之前先取快照。真机上系统把手势 Cancel 掉时，
   * 诊断包里原本只会有 connect_begin 后面接一个 connect_end 也看不到的空洞，
   * 分不清是程序卡死、事件没回来，还是系统正常取消。这里补一条 cancelled 记录。
   * 被 pinch 接管时改为补 superseded_by_pinch：同样要成对，但只记边界，不动全局
   * tracker（当前跑的是 pinch，reset 它等于把刚认领的缩放清掉）。
   */
  private cancelOwnedConnect() {
    const sourcePath: StarMapTargetPath | null = this.connectSourcePath
    const cancelledOwnConnect: boolean = this.gestureTracker.clearIfOwnedBy('connect', this.scenePath)
    if (cancelledOwnConnect && sourcePath !== null) {
      logConnectEnd(sourcePath, null, '', false, 'cancelled')
    } else {
      // 已被 pinch 接管：只补旧 connect 的结束日志，不碰全局 tracker
      this.closeSupersededConnectIfNeeded()
    }
    this.clearConnectVisualState()
  }

  /**
   * 只清连线预览的本地痕迹（起点路径、预览画布、菜单标记）。
   * 这里刻意不碰 gestureTracker：手势可能已经被 pinch 接管。
   */
  private clearConnectVisualState() {
    this.connectSourcePath = null
    this.connectSourceId = ''
    this.deactivatePreviewCanvas()
    this.gestureMode = 'idle'
    this.showNodeMenu = false
    this.nodeMenuNodeId = ''
  }

  /**
   * 单指拖动画布的收尾。
   *
   * 分两件事看：
   * - 「清不清全局 tracker」需要看 ownership。beginPinch 会把 ownerScenePath
   *   也写成归属层，同一层的旧 pan 到这里 isOwnedByScene 仍是 true，所以光靠
   *   owner 判断挡不住，必须靠 clearIfOwnedBy 的 mode 核对（#816）。
   * - 「本层 UI 状态收不收」不看 ownership。旧 handler 已经结束了，本地
   *   gestureMode 没有任何理由继续停在 panCanvas，否则全局是 pinch、本地还是
   *   panCanvas，两边分叉。
   */
  private finishOwnedPan() {
    this.panActive = false
    this.gestureTracker.clearIfOwnedBy('panCanvas', this.scenePath)
    this.gestureMode = 'idle'
  }

  /**
   * 移动节点 / 移动 Embed 的收尾。
   *
   * 只有 mode 和操作对象都还是自己时才落盘；被 pinch 接管时只恢复动画，不落盘
   * 也不清全局。但本层 gestureMode 一定要收回 idle，理由同 finishOwnedPan。
   */
  private finishOwnedMove(moveMode: StarMapGestureMode, itemId: string, saveFn: () => void) {
    const state: GestureState = this.gestureTracker.getState()
    const owns: boolean = state.mode === moveMode && state.activeItemId === itemId &&
      this.gestureTracker.isActiveItemInScene(this.scenePath)
    this.panActive = false
    if (owns) {
      saveFn()
      this.gestureTracker.reset()
    }
    // 动画一定要恢复，否则节点会一直停在 pause 状态（取消路径尤其容易漏）
    this.motionPauseTracker.resumeNode(itemId)
    this.gestureMode = 'idle'
  }

  /**
   * Node 上那个 PanGesture 的收尾入口。
   *
   * 这一个 handler 有两条起手路径：菜单选了"移动"走 moveNode，否则走 panCanvas
   * 拖画布。结束时不能再固定当成 moveNode 收 —— 真走的是 panCanvas 时
   * finishOwnedMove('moveNode', ...) 会 owns=false，tracker 就永远卡在 panCanvas，
   * 之后别层交互全被"不是 idle、owner 也不是我"挡住。
   *
   * 分流看**本层**的 gestureMode，不用当前全局 mode：全局可能已经被 pinch 接管，
   * 那时读全局只会猜不出"当初这个 handler 走的是哪条路"。本层 gestureMode 才是
   * 起手时自己写下的，是唯一可靠的来源（#816）。
   */
  private finishNodePan(nodeId: string, cancelled: boolean) {
    if (this.gestureMode === 'moveNode') {
      this.finishOwnedMove('moveNode', nodeId, cancelled ? () => {} : () => {
        void this.saveNodePosition(nodeId)
      })
      return
    }
    this.finishOwnedPan()
  }

  /** Embed 上那个 PanGesture 的收尾入口，分流理由同 finishNodePan。 */
  private finishEmbedPan(embedInstanceId: string, cancelled: boolean) {
    if (this.gestureMode === 'moveEmbed') {
      this.finishOwnedMove('moveEmbed', embedInstanceId, cancelled ? () => {} : () => {
        void this.saveEmbedPosition(embedInstanceId)
      })
      return
    }
    this.finishOwnedPan()
  }

  /**
   * 落一条边：通过 bridge 的星图 CRUD API 写回**宿主 Scene**的图。
   *
   * 端点路径已经由 planCrossLayerEdge 换算成相对本 Scene，所以这里直接透传，
   * 不再做任何坐标或层级判断。边一定存进宿主那张图，这是 Core 的既有语义
   * （facade/starmap_ops/graph.rs::add_starmap_edge 把边写进 starmap_id 自己的图）。
   */
  async createEdgeBetween(from: StarMapTargetPath, to: StarMapTargetPath): Promise<boolean> {
    if (!this.bridge) {
      return false
    }
    const result = await this.bridge.addStarMapEdge(this.starmapId, from, to, 'RelatedTo', null)
    if (result.success && result.data) {
      // 成功分支必须留痕：Issue #813 的诊断包里只有 create_starmap_child_embed，
      // 无法判断是"没画线"还是"画了线但没落库"。这条日志同时覆盖普通节点和 Embed 端点。
      hilog.info(
        0x0000, 'StarMapScene',
        `createEdge ok: starmapId=${this.starmapId}, edgeId=${result.data.id}`
      )
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: [...this.graph.edges, result.data],
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.recomputeGeometry()
      }
      this.onCreateEdge(this.scenePath, this.starmapId, from.target.nodeId ?? '', to.target.nodeId ?? '')
      return true
    }
    hilog.error(0x0000, 'StarMapScene', `createEdge failed: ${result.errorCode}, ${result.userMessage}`)
    return false
  }

  /**
   * 保存节点位置：通过 bridge 的星图节点更新 API 写回。
   * 使用 this.starmapId。
   */
  async saveNodePosition(nodeId: string) {
    const ln = this.getPlatformLayoutNode(nodeId)
    if (!ln || !this.bridge) return
    hilog.info(0x0000, 'StarMapScene', `saveNodePosition: ${nodeId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapNode(this.starmapId, nodeId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveNodePosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveNode(this.scenePath, this.starmapId, nodeId, { x: ln.x, y: ln.y })
  }

  /**
   * 保存 Embed 位置：通过 bridge 的 Embed update API 写回。
   * 使用 this.starmapId。
   */
  async saveEmbedPosition(embedId: string) {
    const ln = this.getPlatformLayoutNode(embedId)
    if (!ln || !this.bridge) return
    hilog.info(0x0000, 'StarMapScene', `saveEmbedPosition: ${embedId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapEmbed(this.starmapId, embedId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveEmbedPosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveEmbed(this.scenePath, this.starmapId, embedId, { x: ln.x, y: ln.y })
  }

  /**
   * 执行新建节点：通过 bridge API 创建，使用 this.starmapId。
   * 标题由窗口层级的对话框通过参数传入。
   */
  async performCreateNode(title: string) {
    const trimmed: string = title.trim()
    if (trimmed.length === 0) return
    if (!this.bridge) return
    const position: StarMapPoint = {
      x: this.createMenuCanvasPosition.x,
      y: this.createMenuCanvasPosition.y
    }
    const result = await this.bridge.addStarMapNode(this.starmapId, trimmed, 'Custom', position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: [...this.graph.nodes, result.data],
          edges: this.graph.edges,
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
        this.recomputeGeometry()
      }
      this.selectItem('node', result.data.id)
      this.onCreateNode(this.scenePath, this.starmapId, trimmed, position)
      logCreateResult('create_node', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, true, `id=${result.data.id}`)
    } else {
      hilog.error(0x0000, 'StarMapScene', `addStarMapNode failed: ${result.errorCode}, ${result.userMessage}`)
      logCreateResult('create_node', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, false, `${result.errorCode}, ${result.userMessage}`)
    }
  }

  /**
   * 执行新建子星图：通过 bridge API 创建，使用 this.starmapId。
   * 标题由窗口层级的对话框通过参数传入。
   */
  async performCreateEmbed(title: string) {
    const trimmed: string = title.trim()
    if (trimmed.length === 0) return
    if (!this.bridge) return
    const position: StarMapPoint = {
      x: this.createMenuCanvasPosition.x,
      y: this.createMenuCanvasPosition.y
    }
    const result = await this.bridge.createStarmapChildEmbed(this.starmapId, trimmed, position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: this.graph.edges,
          embeds: [...this.graph.embeds, result.data.embed],
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds)
        this.recomputeGeometry()
      }
      this.onCreateEmbed(this.scenePath, this.starmapId, trimmed, position)
      logCreateResult('create_embed', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, true, `instanceId=${result.data.embed.instanceId}`)
    } else {
      hilog.error(0x0000, 'StarMapScene', `createStarmapChildEmbed failed: ${result.errorCode ?? 'unknown'}`)
      logCreateResult('create_embed', describeScenePath(this.scenePath), this.starmapId,
        trimmed, position.x, position.y, false, `${result.errorCode ?? 'unknown'}`)
    }
  }

  // ─── 窗口层级对话框 ───

  /**
   * 打开标题输入对话框。
   * 对话框挂在窗口层级（openCustomDialog），所以不会被圆形 Embed 的 clip(true) 裁掉。
   * 连续点击时先关掉旧对话框，避免叠多层。
   */
  private openCreateDialog(args: CreateTitleDialogArgs, isEmbedDialog: boolean) {
    const content: ComponentContent<CreateTitleDialogArgs> = createTitleDialogContent(this.getUIContext(), args)
    if (isEmbedDialog) {
      if (this.createEmbedDialogContent !== null) {
        this.getUIContext().getPromptAction().closeCustomDialog(this.createEmbedDialogContent)
      }
      this.createEmbedDialogContent = content
    } else {
      if (this.createNodeDialogContent !== null) {
        this.getUIContext().getPromptAction().closeCustomDialog(this.createNodeDialogContent)
      }
      this.createNodeDialogContent = content
    }
    // openCustomDialog 的 ComponentContent 版本完全跟随内容样式（等价 customStyle=true），
    // 对话框本身不画背景，遮罩由 maskColor 控制。
    this.getUIContext().getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      autoCancel: true,
      onDidDisappear: () => {
        if (this.createNodeDialogContent === content) {
          this.createNodeDialogContent = null
        }
        if (this.createEmbedDialogContent === content) {
          this.createEmbedDialogContent = null
        }
      }
    })
  }

  /** 关闭本 Scene 打开的所有对话框（Scene 消失时也调用）。 */
  private closeCreateDialogs() {
    if (this.createNodeDialogContent !== null) {
      this.getUIContext().getPromptAction().closeCustomDialog(this.createNodeDialogContent)
      this.createNodeDialogContent = null
    }
    if (this.createEmbedDialogContent !== null) {
      this.getUIContext().getPromptAction().closeCustomDialog(this.createEmbedDialogContent)
      this.createEmbedDialogContent = null
    }
  }

  /** 打开新建节点对话框。 */
  private openCreateNodeDialog() {
    this.openCreateDialog({
      title: '新建节点',
      placeholder: '请输入节点名称',
      onConfirm: (title: string) => {
        this.closeCreateDialogs()
        void this.performCreateNode(title)
      },
      onCancel: () => {
        this.closeCreateDialogs()
      }
    }, false)
  }

  /** 打开新建子星图对话框。 */
  private openCreateEmbedDialog() {
    this.openCreateDialog({
      title: '新建子星图',
      placeholder: '请输入子星图名称',
      onConfirm: (title: string) => {
        this.closeCreateDialogs()
        void this.performCreateEmbed(title)
      },
      onCancel: () => {
        this.closeCreateDialogs()
      }
    }, true)
  }

  // ─── Canvas 绘制 ───

  private deactivatePreviewCanvas(): void {
    this.previewCanvasReady = false
  }

  /**
   * 绘制边：使用本地 localEdgeRenders。
   * wobble 偏移在节点位置计算中已包含，此处只需应用 zoom/offset 变换。
   */
  drawEdges() {
    if (!this.canvasActive || !this.canvasReady || !this.graph) return
    const ctx = this.canvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    const progress = this.motionProgress
    const wobbleMap = new Map<string, WobbleOffset>()
    for (const node of this.graph.nodes) {
      const dx = wobbleXFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      const dy = wobbleYFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      wobbleMap.set(node.id, { dx, dy })
    }

    for (const edge of this.localEdgeRenders) {
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = resolveEdgeEndpointAnchor(graphEdge.from, this.starmapId) ?? ''
      const toId = resolveEdgeEndpointAnchor(graphEdge.to, this.starmapId) ?? ''
      const defaultWobble: WobbleOffset = { dx: 0, dy: 0 }
      const fromWobble = wobbleMap.get(fromId) || defaultWobble
      const toWobble = wobbleMap.get(toId) || defaultWobble

      const sx = (edge.startX + fromWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const sy = (edge.startY + fromWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const ex = (edge.endX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const ey = (edge.endY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()

      // 选中的边用 (scenePath, kind, itemId) 判定，不看端点 id：
      // 同层里"起点是我选中的节点"和"这条边本身被选中"是两件事。
      const isSelected = this.selectionState.isSelected(this.scenePath, 'edge', graphEdge.id)

      ctx.beginPath()
      ctx.moveTo(sx, sy)
      ctx.lineTo(ex, ey)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5

      if (graphEdge.kind === 'DependsOn' || graphEdge.kind === 'ConflictsWith') {
        ctx.setLineDash([6, 4])
      } else {
        ctx.setLineDash([])
      }
      ctx.stroke()
      ctx.setLineDash([])

      // 箭头
      const atx = (edge.arrowTipX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const aty = (edge.arrowTipY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const alx = (edge.arrowLeftX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const aly = (edge.arrowLeftY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()
      const arx = (edge.arrowRightX + toWobble.dx) * this.viewportScaleValue() + this.viewportOffsetX()
      const ary = (edge.arrowRightY + toWobble.dy) * this.viewportScaleValue() + this.viewportOffsetY()

      ctx.beginPath()
      ctx.moveTo(atx, aty)
      ctx.lineTo(alx, aly)
      ctx.moveTo(atx, aty)
      ctx.lineTo(arx, ary)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5
      ctx.stroke()

      // 边标签
      if (graphEdge.label && edge.labelX !== undefined && edge.labelY !== undefined) {
        const lx = (edge.labelX + (fromWobble.dx + toWobble.dx) / 2) * this.viewportScaleValue() + this.viewportOffsetX()
        const ly = (edge.labelY + (fromWobble.dy + toWobble.dy) / 2) * this.viewportScaleValue() + this.viewportOffsetY()
        ctx.font = '11px HarmonyOS Sans'
        ctx.fillStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.onSurfaceVariant
        ctx.textAlign = 'center'
        ctx.textBaseline = 'bottom'
        ctx.fillText(graphEdge.label, lx, ly - 6)
      }
    }
  }

  /**
   * 绘制预览连线：connect 模式下从源节点到当前手指位置。
   */
  drawPreviewLine() {
    if (!this.canvasActive || !this.previewCanvasReady) return
    const ctx = this.previewCanvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    ctx.beginPath()
    ctx.moveTo(this.previewLineFrom.x, this.previewLineFrom.y)
    ctx.lineTo(this.previewLineTo.x, this.previewLineTo.y)
    ctx.strokeStyle = this.themeConfig.colors.primary
    ctx.lineWidth = 2
    ctx.setLineDash([8, 4])
    ctx.stroke()
    ctx.setLineDash([])
  }

  // ─── 节点颜色 ───

  getNodeColor(node: StarMapNode): string {
    const kindColors: Record<string, string> = {
      'Character': '#4A90D9',
      'Location': '#F39C12',
      'Event': '#9B59B6',
      'Theme': '#2ECC71',
      'Plot': '#E74C3C',
      'Item': '#1ABC9C',
      'Organization': '#3498DB',
      'Custom': '#95A5A6'
    }
    return kindColors[node.kind] || '#4A90D9'
  }

  // ─── @Builder：节点组件 ───

  /**
   * 节点组件：长按 → 菜单（连线预备态）→ 继续拖动 → connect。
   * 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath。
   */
  @Builder
  StarMapNodeItem(node: StarMapNode) {
    Column() {
      Circle({ width: 56 * this.viewportScaleValue(), height: 56 * this.viewportScaleValue() })
        .fill(this.getNodeColor(node))
        .stroke(this.isNodeSelected(node.id) ? STARMAP_SELECTED_STROKE_COLOR : 'transparent')
        .strokeWidth(4)
        .shadow({
          radius: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.radius :
            (this.isNodeSelected(node.id) ? DesignTokens.shadowMedium.radius : DesignTokens.shadowLight.radius),
          color: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            '#40000000' :
            (this.isNodeSelected(node.id) ? '#40FF6B6B' : '#20000000'),
          offsetX: 0,
          offsetY: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.offsetY : DesignTokens.shadowLight.offsetY
        })

      Text(node.title)
        .fontSize(13 * this.viewportScaleValue())
        .fontWeight(this.isNodeSelected(node.id) ? FontWeight.Bold : FontWeight.Normal)
        .margin({ top: 6 * this.viewportScaleValue() })
        .maxLines(2)
        .textAlign(TextAlign.Center)
        .width(80 * this.viewportScaleValue())
    }
    .width(160 * this.viewportScaleValue())
    .height(80 * this.viewportScaleValue())
    .justifyContent(FlexAlign.Center)
    .position({
      x: this.getNodeScreenX(node.id),
      y: this.getNodeScreenY(node.id)
    })
    .scale(
      this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id
        ? { x: this.motionConfig.dragLiftScale, y: this.motionConfig.dragLiftScale }
        : { x: 1, y: 1 }
    )
    .onClick(() => {
      this.selectItem('node', node.id)
    })
    // 菜单不挂在本 Column 上：bindMenu 按被绑定组件定位，节点本体位置会随节点移动而变化，
    // 而且 Scene 栈里的 1vp 锚点才能让菜单贴着实际长按的手指位置弹出（见 build() 的锚点）。
    // 这里只负责翻转 showNodeMenu 并记下锚点，菜单内容通过 this.nodeMenuNodeId 区分节点 / Embed。
    .gesture(
      GestureGroup(GestureMode.Exclusive,
        // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
        GestureGroup(GestureMode.Sequence,
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              this.gestureTracker.beginNodeMenu(this.scenePath, node.id, point.x, point.y)
              this.gestureMode = 'nodeMenu'
              // 先记录目标再翻显示标记：菜单内容要在 showNodeMenu 变 true 时就读到 nodeMenuNodeId
              this.nodeMenuNodeId = node.id
              // 锚点记手指实际位置，菜单从这儿弹出而不是从节点中心弹出
              this.nodeMenuAnchor = { x: point.x, y: point.y }
              this.showNodeMenu = true
              this.selectItem('node', node.id)
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              // 起点存完整路径，跨层连线要靠它和终点求最近公共祖先（#816）
              this.beginConnectFrom(node.id, point)
            })
            .onActionUpdate((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              const state = this.gestureTracker.getState()
              if (state.mode === 'connect') {
                this.previewLineTo = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
                // 起点随手指方向实时重算，始终贴在源对象边界上
                this.updatePreviewLineFrom(node.id, this.previewLineTo)
                this.drawPreviewLine()
              }
            })
            .onActionEnd(() => {
              // 统一收尾：mode 已经不是 connect（被 pinch 接管）时只清预览，不碰全局 tracker
              this.finishOwnedConnect()
            })
            .onActionCancel(() => {
              // 触摸被系统取消：仍是自己的 connect 就清回 idle，已被 pinch 接管就只清预览（#816）
              this.cancelOwnedConnect()
            })
        ),
        // 没有长按成功时的普通滑动
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              this.motionPauseTracker.pauseNode(node.id)
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.moveNodeStartX = ln.x
                this.moveNodeStartY = ln.y
              }
            } else {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseX = this.viewportOffsetX()
              this.panBaseY = this.viewportOffsetY()
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              const newDx = this.moveNodeStartX + event.offsetX / this.viewportScaleValue()
              const newDy = this.moveNodeStartY + event.offsetY / this.viewportScaleValue()
              const positions = new Map<string, PlatformPoint>()
              positions.set(node.id, { x: newDx, y: newDy })
              this.platformLayoutNodes = applyFreeformLayout(this.platformLayoutNodes, positions)
              this.recomputeGeometry()
            } else if (state.mode === 'panCanvas' && this.panActive &&
              this.gestureTracker.isPanOwnedByScene(this.scenePath) && this.canMutateViewport()) {
              this.setViewportOffset(this.panBaseX + event.offsetX, this.panBaseY + event.offsetY)
            }
          })
          .onActionEnd(() => {
            // 起手可能是移动节点，也可能是拖画布，按本地 gestureMode 分流收尾
            this.finishNodePan(node.id, false)
          })
          .onActionCancel(() => {
            this.finishNodePan(node.id, true)
          })
      )
    )
  }

  // ─── Embed 手势组 ───

  /**
   * Embed 手势组：选中/菜单/连线/移动/画布拖动。
   * title Row 和边框命中条共享此手势组。
   * Pan 手势：moveEmbed 模式时移动 Embed，否则按 #373 规则拖动当前 Scene 画布。
   * innerContent 不挂父手势，由 child Scene 自己处理。
   * 所有 onActionStart 检查 isOwnedByScene，防止别层正在用手势时覆盖 owner。
   *
   * 单击和双击都必须落在同一个 Embed 选中上（#816）：双击声明在单击之前，
   * 双击的第二次按下不触发单击识别，所以单击不会被双击抢走。
   * 两者都只是选中这个 Embed，都不开新页面。
   */
  private embedGestureGroup(embed: StarMapEmbed): GestureGroupInterface {
    return GestureGroup(GestureMode.Exclusive,
      // 双击 → 选中 Embed（和单击同一个结果）
      TapGesture({ count: 2 })
        .onAction(() => {
          this.selectItem('embed', embed.instanceId)
        }),
      // 单击 → 选中 Embed
      TapGesture({ count: 1 })
        .onAction(() => {
          this.selectItem('embed', embed.instanceId)
        }),
      // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
      GestureGroup(GestureMode.Sequence,
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.gestureTracker.beginNodeMenu(this.scenePath, embed.instanceId, point.x, point.y)
            this.gestureMode = 'nodeMenu'
            // 菜单内容在 showNodeMenu 翻 true 的同一帧就要读到 nodeMenuNodeId，所以先赋值再翻标记
            this.nodeMenuNodeId = embed.instanceId
            // 和普通节点共用同一个锚点：长按点边框时菜单也从实际按下处弹出
            this.nodeMenuAnchor = { x: point.x, y: point.y }
            this.showNodeMenu = true
          }),
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.beginConnectFrom(embed.instanceId, point)
          })
          .onActionUpdate((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              this.previewLineTo = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
              // Embed 起点同样走圆周交点，不会再从黑圆中心拉出虚线
              this.updatePreviewLineFrom(embed.instanceId, this.previewLineTo)
              this.drawPreviewLine()
            }
          })
          .onActionEnd(() => {
            // 与 Node 的 connect 走同一条收尾：被 pinch 接管时只清预览
            this.finishOwnedConnect()
          })
          .onActionCancel(() => {
            // 与 Node 的 connect 走同一条取消收尾
            this.cancelOwnedConnect()
          })
      ),
      // 没有长按成功时的普通滑动（moveEmbed 或画布拖动，与 #373 规则一致）
      PanGesture()
        .onActionStart((event: GestureEvent) => {
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            this.motionPauseTracker.pauseNode(embed.instanceId)
            const ln = this.getPlatformLayoutNode(embed.instanceId)
            if (ln) {
              this.moveEmbedStartX = ln.x
              this.moveEmbedStartY = ln.y
            }
          } else {
            if (this.gestureTracker.isPinching()) return
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            this.panActive = true
            const point = this.gestureScreenPoint(event, 0, 0)
            this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
            this.gestureMode = 'panCanvas'
            this.panBaseX = this.viewportOffsetX()
            this.panBaseY = this.viewportOffsetY()
          }
        })
        .onActionUpdate((event: GestureEvent) => {
          if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            const newDx = this.moveEmbedStartX + event.offsetX / this.viewportScaleValue()
            const newDy = this.moveEmbedStartY + event.offsetY / this.viewportScaleValue()
            const positions = new Map<string, PlatformPoint>()
            positions.set(embed.instanceId, { x: newDx, y: newDy })
            this.embedLayoutNodes = applyFreeformLayout(this.embedLayoutNodes, positions)
            this.recomputeGeometry()
          } else if (state.mode === 'panCanvas' && this.panActive &&
            this.gestureTracker.isPanOwnedByScene(this.scenePath) && this.canMutateViewport()) {
            this.setViewportOffset(this.panBaseX + event.offsetX, this.panBaseY + event.offsetY)
          }
        })
        .onActionEnd(() => {
          // 与 Node 同一个分流：起手可能是移动 Embed，也可能是拖画布
          this.finishEmbedPan(embed.instanceId, false)
        })
        .onActionCancel(() => {
          this.finishEmbedPan(embed.instanceId, true)
        })
    )
  }

  // ─── @Builder：Embed 组件 ───

  /**
   * Embed 组件：黑色正圆，画布上唯一的外接形状是圆。
   *
   * 尺寸只有一个来源：DEFAULT_EMBED_DIAMETER（见 StarMapLayout），
   * 宽=高=直径，圆角=直径/2。这里禁止再出现 200 / 120 / 12 这类手写尺寸。
   *
   * 命中分层不再用"三根矩形窄条 + clip 裁圆环"：clip(true) 只会把矩形条裁成圆内的几段直线，
   * 斜着点边框（例如 45° 方向）既不在 title 也不在任何窄条上，事件会掉进 child Scene，
   * 于是"几何算出来是 embedBorder、实际却进了子图"。现在两层一起改：
   * - responseRegion 把真实触摸热区切成圆（默认热区是整个矩形，clip 不裁热区）
   * - 外层 Stack 的 onTouchIntercept 按真实圆距离动态分流：
   *   title 条或真实圆环 → HitTestMode.Block，只给外层 Embed，child Scene 不参与
   *   圆外 / 圆内其余区域 → HitTestMode.None，事件落回父星图或交给 child StarMapScene
   */
  @Builder
  StarMapEmbedItem(embed: StarMapEmbed) {
    Stack() {
      // 1. innerContent 区域（占满整个 Embed 内容区，放 child StarMapEmbedScene）
      // 不挂任何手势，能不能收到事件由下面的 onTouchIntercept 决定
      Column() {
        StarMapEmbedScene({
          rootStarmapId: this.rootStarmapId,
          scenePath: this.scenePath,
          embed: embed,
          // 子 Scene 的深度是父层 +1：只有根 Scene 是全局相机本体（#818）
          sceneDepth: this.sceneDepth + 1,
          themeConfig: this.themeConfig,
          motionConfig: this.motionConfig,
          motionProgress: this.motionProgress,
          bridge: this.bridge,
          gestureTracker: this.gestureTracker,
          // 全树共用的选中状态和 Scene 注册表
          selectionState: this.selectionState,
          sceneRegistry: this.sceneRegistry,
          // 全局相机原样透传：子 Scene 是相机里的内容，不持有自己的缩放（#818）
          cameraScale: this.cameraScale,
          cameraOffsetX: this.cameraOffsetX,
          cameraOffsetY: this.cameraOffsetY,
          onCameraChanged: this.onCameraChanged,
        })
      }
      .width('100%')
      .height('100%')

      // 2. title 视觉层（纯显示，不挂手势也不挂菜单）
      // 手势统一由外层 Stack 的 embedGestureGroup + onTouchIntercept 分流，
      // 菜单由 Scene 栈里的 1vp 锚点承载（菜单若挂在这个 title 上，
      // 位置会被节点本体绑死，点圆环时菜单不从按下处弹出）。
      Row() {
        Text(embed.label ?? embed.targetStarmapId)
          .fontSize(14 * this.viewportScaleValue())
          .fontWeight(FontWeight.Medium)
          // 跟随主题：外壳底色已经是主题 surfaceContainer，写死白色在浅色主题下会看不清（#818）
          .fontColor(this.themeConfig.colors.onSurface)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          // 圆顶部的可见宽度远小于直径：左对齐 + padding 的文字前半截会被圆裁掉，必须居中
          .width(80 * this.viewportScaleValue())
          .textAlign(TextAlign.Center)
      }
      .width('100%')
      .height(EMBED_TITLE_HIT_HEIGHT * this.viewportScaleValue())
      .justifyContent(FlexAlign.Center)
      .alignItems(VerticalAlign.Center)
      .position({ x: 0, y: 0 })
      .hitTestBehavior(HitTestMode.None)
    }
    .width(DEFAULT_EMBED_DIAMETER * this.viewportScaleValue())
    .height(DEFAULT_EMBED_DIAMETER * this.viewportScaleValue())
    .borderRadius(DEFAULT_EMBED_DIAMETER / 2 * this.viewportScaleValue())
    // 跟随主题：写死黑色时浅色主题下整颗子星图是一块黑饼，标题白字也压不出来（#818）
    .backgroundColor(this.themeConfig.colors.surfaceContainer)
    // 未选中也要有一条细描边，否则圆壳在同色底上看不出边界；选中时加粗。
    // 选中色用 STARMAP_SELECTED_STROKE_COLOR（和普通节点一致），不能取 themeConfig.colors.primary：
    // 主色跟着配色走，可能和外壳底色接近，选中态就糊了。
    .border({
      width: this.isEmbedSelected(embed.instanceId) ? 3 : 1,
      color: this.isEmbedSelected(embed.instanceId)
        ? STARMAP_SELECTED_STROKE_COLOR
        : this.themeConfig.colors.outlineVariant
    })
    .position({
      x: this.getEmbedScreenX(embed.instanceId),
      y: this.getEmbedScreenY(embed.instanceId)
    })
    // 子 Scene 必须真正被裁到圆内，否则内部会溢出圆外看起来还是方形
    .clip(true)
    // 圆形热区：ArkUI 组件默认热区是整个矩形，borderRadius + clip 只裁视觉不裁热区，
    // 不显式声明的话圆外四个方角依然可点。切成若干条水平带逼近圆（见 buildCircleResponseBands）。
    .responseRegion(this.buildEmbedCircleResponseRegions())
    .gesture(this.embedGestureGroup(embed))
    // 动态事件分流（坐标是相对本组件的局部坐标，圆心即 直径/2）：
    // - 圆外：不属于 Embed，热区已排除，这里再兜一层，事件落回父星图
    // - 圆内 title / 真实圆环：Block，只给外层 Embed，不再让 child Scene 参与手势竞争
    //   （Default 是"自己和子节点都参加"，父 Embed 和 child Scene 会一起进竞争）
    // - 圆内其余：None，交给 child StarMapScene
    .onTouchIntercept((event: TouchEvent): HitTestMode => {
      const touch = event.changedTouches[0]
      if (!touch) {
        return HitTestMode.None
      }
      // 和 trackFinger 同一个坑：相对坐标字段在真机上不总是有值，
      // 这里也只吃有限数，缺了就直接放行，不要拿 NaN 去算圆环命中。
      const localX: number = touch.x
      const localY: number = touch.y
      if (!Number.isFinite(localX) || !Number.isFinite(localY)) {
        return HitTestMode.None
      }
      const diameter: number = DEFAULT_EMBED_DIAMETER * this.viewportScaleValue()
      const radius: number = diameter / 2
      const dx: number = localX - radius
      const dy: number = localY - radius
      const dist: number = Math.sqrt(dx * dx + dy * dy)
      if (dist > radius) {
        return HitTestMode.None
      }
      const inTitle: boolean = localY <= EMBED_TITLE_HIT_HEIGHT * this.viewportScaleValue()
      const inBorder: boolean = dist >= radius - EMBED_BORDER_HIT_WIDTH * this.viewportScaleValue()
      return (inTitle || inBorder) ? HitTestMode.Block : HitTestMode.None
    })
  }

  /**
   * 生成 Embed 圆形的真实触摸热区：把圆切成若干条水平矩形带。
   * 几何在 StarMapGeometry.buildCircleResponseBands（纯函数，可测），
   * 这里只负责转成 ArkUI 的 Rectangle。
   */
  private buildEmbedCircleResponseRegions(): Rectangle[] {
    const bands = buildCircleResponseBands(DEFAULT_EMBED_DIAMETER * this.viewportScaleValue())
    const regions: Rectangle[] = []
    for (const band of bands) {
      regions.push({
        x: band.x,
        y: band.y,
        width: band.width,
        height: band.height
      })
    }
    return regions
  }

  // ─── 菜单显示状态回写 ───

  /**
   * 节点/Embed 菜单消失后的状态回写（MenuOptions.onDisappear）。
   *
   * 程序化 bindMenu(isShow, ...) 的 isShow 不会自动双向同步：用户点菜单外部关掉菜单后，
   * showNodeMenu 可能还停在 true。下一次长按同一个对象再赋一次 true，状态没有变化，
   * 菜单就可能不再重新打开。所以必须在这里把状态写回。
   *
   * 不能无脑 reset 手势：长按后继续拖动（进 connect）和点"移动"（进 moveNode/moveEmbed）
   * 也会主动关菜单，这两种情况下 gestureMode 已经不是 nodeMenu，所以只在 nodeMenu 态才 reset。
   */
  private onNodeMenuDisappear() {
    this.showNodeMenu = false
    // 本地 gestureMode 和全局 mode 都得是 nodeMenu 才清全局：
    // 只看本地的话，本地标记还没更新到位就会把别的手势（尤其是刚认领的 pinch）清掉。
    if (this.gestureTracker.clearIfOwnedBy('nodeMenu', this.scenePath)) {
      this.gestureMode = 'idle'
    }
    this.nodeMenuNodeId = ''
  }

  /** 新建菜单消失后的状态回写（MenuOptions.onDisappear）。 */
  private onCreateMenuDisappear() {
    this.showCreateMenu = false
  }

  // ─── @Builder：节点菜单（bindMenu 内容） ───

  /**
   * 节点/Embed 菜单内容：交给 ArkUI 程序化 bindMenu 渲染。
   * 不再用 Column + Button + .position() 自绘弹层：自绘弹层是递归 Scene 的普通子组件，
   * 会被圆形 Embed 的 clip(true) 裁掉，也会因为用 Scene 局部坐标定位而越出屏幕（Issue #813）。
   * 程序化 bindMenu 由系统定位并自动避让屏幕边缘，菜单挂在哪个组件上就出现在哪附近。
   *
   * bindMenu 只负责"显示"：长按和后续 Pan 仍然由 GestureGroup 状态机负责，
   * 所以 #373 的"长按出菜单、继续移动拉线"不受影响。
   */
  @Builder
  NodeMenuContent() {
    Menu() {
      if (this.isEmbed(this.nodeMenuNodeId)) {
        MenuItem({ content: $r('app.string.move_embed') })
          .onClick(() => {
            this.gestureTracker.beginMoveEmbed(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveEmbed'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      } else {
        MenuItem({ content: $r('app.string.move_node') })
          .onClick(() => {
            this.gestureTracker.beginMoveNode(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveNode'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      }
    }
  }

  // ─── @Builder：新建菜单（bindMenu 内容） ───

  /**
   * 空白处新建菜单内容：只有"新建节点"和"新建子星图"两项。
   * 点击后先关菜单，再打开窗口层级的对话框（不会被 Embed 的 clip 裁掉）。
   */
  @Builder
  CreateMenuContent() {
    Menu() {
      MenuItem({ content: $r('app.string.create_node') })
        .onClick(() => {
          this.showCreateMenu = false
          this.openCreateNodeDialog()
        })

      MenuItem({ content: $r('app.string.create_embed') })
        .onClick(() => {
          this.showCreateMenu = false
          this.openCreateEmbedDialog()
        })
    }
  }

  // ─── 辅助方法 ───

  /**
   * 获取当前星图所有 Embed 的 instanceId 集合，用于 hitTestWithScene 区分节点和 Embed。
   */
  private getEmbedInstanceIds(): Set<string> {
    if (!this.graph) return new Set()
    const ids = new Set<string>()
    for (const embed of this.graph.embeds) {
      ids.add(embed.instanceId)
    }
    return ids
  }

  // ─── build ───

  build() {
    Stack() {
      // 0. 背景命中层（只处理当前 Scene 空白区域的 Pinch/Pan/LongPress）
      // 位于 Stack 最底层，子组件在上层优先接收触摸事件
      // 空白区域的触摸事件穿透到此层
      Column() {}
      .width('100%')
      .height('100%')
      .gesture(
        // 空白区域手势：Pan（画布拖动）+ LongPress（长按新建）
        //
        // Pinch 不在这里（#816）：这个 Column 只是 Stack 里的一个兄弟节点，
        // 手指落在子星图上时根本收不到事件，所以双指缩放以前会随机失效。
        // 缩放统一交给根 Scene 栈上的 onTouch，那里能看到所有层的全部手指。
        GestureGroup(GestureMode.Parallel,
          TapGesture()
            .onAction((event: GestureEvent) => {
              // 空白层单击：选中递归命中的目标（含子层里的边），没有目标就清选中。
              // 节点 / Embed 在 Stack 上层，事件被它们收走，不会走到这里（#816）。
              const point = this.gestureScreenPoint(event, 0, 0)
              this.handleBlankTap(point.x, point.y)
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (this.gestureTracker.isPinching()) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseX = this.viewportOffsetX()
              this.panBaseY = this.viewportOffsetY()
            })
            .onActionUpdate((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              if (!this.panActive || this.gestureTracker.isPinching()) return
              if (!this.canMutateViewport()) return
              if (this.gestureMode === 'panCanvas') {
                this.setViewportOffset(this.panBaseX + event.offsetX, this.panBaseY + event.offsetY)
              }
            })
            .onActionEnd(() => {
              // mode 还是 panCanvas 且归属本层才清全局；被 pinch 接管时不动它
              this.finishOwnedPan()
            })
            .onActionCancel(() => {
              this.finishOwnedPan()
            }),
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, 0, 0)
              const canvasPoint = screenToCanvas(point.x, point.y, this.viewportScaleValue(), this.viewportOffsetX(), this.viewportOffsetY())
              // 空白判定和选中/连线共用同一个递归命中：点在子星图内部时父 Scene 不该弹新建菜单
              const context: RecursiveSceneContext | null = this.getRootContext()
              let isBlank: boolean = false
              if (context !== null) {
                const rootPoint: PlatformPoint = convertPointToRoot(context, this.scenePath, point.x, point.y)
                isBlank = resolveRecursiveHit(context, rootPoint.x, rootPoint.y).target === null
              }
              // 空白区域长按 → 弹出新建菜单。
              // 菜单挂在 Scene 栈里的 1vp 锚点上（不挂在本背景层）：
              // bindMenu 按被绑定组件定位，挂全屏背景层会让菜单相对整张 Scene 放，
              // 不再相对手指长按的位置放（Issue #813 复审）。
              if (isBlank) {
                this.createMenuAnchor = { x: point.x, y: point.y }
                this.createMenuCanvasPosition = { x: canvasPoint.x, y: canvasPoint.y }
                this.showCreateMenu = true
              }
            })
        )
      )
      .hitTestBehavior(HitTestMode.Default)

      // 1. Canvas：绘制当前层的边（只负责画，不参与触摸测试）
      Canvas(this.canvasCtx)
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .onReady(() => {
          this.canvasReady = true
          this.drawEdges()
        })

      // 2. 节点层
      if (this.graph) {
        ForEach(this.graph.nodes, (node: StarMapNode) => {
          this.StarMapNodeItem(node)
        }, (node: StarMapNode) => node.id)

        // 3. Embed 层
        ForEach(this.graph.embeds, (embed: StarMapEmbed) => {
          this.StarMapEmbedItem(embed)
        }, (embed: StarMapEmbed) => embed.instanceId)
      }

      // 4. 预览连线 Canvas（connect 模式，只负责画，不参与触摸测试）
      if (this.gestureMode === 'connect') {
        Canvas(this.previewCanvasCtx)
          .width('100%')
          .height('100%')
          .hitTestBehavior(HitTestMode.None)
          .onReady(() => {
            this.previewCanvasReady = true
            this.drawPreviewLine()
          })
      }

      // 5. 菜单锚点：1vp×1vp 透明锚点，hitTestBehavior(None) 保证它自己不吃事件，
      // 只用来给 bindMenu 提供"贴着手指弹出"的定位基准。
      // 菜单浮层由系统绘制并自动避让屏幕边缘，锚点本身在圆形 Embed 内也不会被 clip(true) 裁掉。
      // onDisappear 必须回写显示标记：程序化 bindMenu 的 isShow 不会自动同步，
      // 不回写的话第二次长按同一对象会因状态没变化而弹不出来（Issue #813 复审）。
      Row() {}
        .width(1)
        .height(1)
        .position({ x: this.nodeMenuAnchor.x, y: this.nodeMenuAnchor.y })
        .hitTestBehavior(HitTestMode.None)
        .bindMenu(this.showNodeMenu, this.NodeMenuContent, {
          onDisappear: () => {
            this.onNodeMenuDisappear()
          }
        })

      Row() {}
        .width(1)
        .height(1)
        .position({ x: this.createMenuAnchor.x, y: this.createMenuAnchor.y })
        .hitTestBehavior(HitTestMode.None)
        .bindMenu(this.showCreateMenu, this.CreateMenuContent, {
          onDisappear: () => {
            this.onCreateMenuDisappear()
          }
        })

      // 对话框不走这里：新建对话框挂在窗口层级（openCustomDialog）。
      // 菜单和对话框本体都不放进这个 Stack，自绘弹层会被圆形 Embed 的 clip(true) 裁掉（Issue #813）。
    }
    .width('100%')
    .height('100%')
    // onTouch 只挂在根 Scene 的栈上（#816）：
    // 子 Scene 的栈同样会收到事件，但 sceneDepth !== 0 的分支直接 return，
    // 所有双指逻辑都在这一处发生，父层不可能绕过 owner 改自己的视口。
    .onTouch((event: TouchEvent) => {
      this.onSceneTouch(event)
    })
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      this.sceneWidth = Number(newArea.width)
      this.sceneHeight = Number(newArea.height)
      // 记下本 Scene 在屏幕上的原点：ArkUI 只保证 screenX / screenY 有值，
      // 触摸点的相对坐标字段在真机上并不总是有值，缺了就得靠这个原点换算回来。
      const globalPosition: Position | undefined = newArea.globalPosition
      if (globalPosition !== undefined && globalPosition !== null) {
        this.sceneGlobalX = Number(globalPosition.x)
        this.sceneGlobalY = Number(globalPosition.y)
      }
      // 第一次到位 = 首次适配；之后尺寸变化 = 父层缩放带动容器变大变小，
      // 要把已有适配同比带过去，而不是重新适配（重新适配会重置用户刚捏出来的缩放）
      if (this.hasFittedView) {
        this.syncFitToSceneSize()
      } else {
        this.maybeFitView()
      }
    })
  }
}
