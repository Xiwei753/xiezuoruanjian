// StarMapProofReplayPanel.ets — Issue #1036 星图创作记录只读回放（HarmonyOS）。
//
// 只消费 Core 的历史帧 JSON：节点、连线、Embed、子图和事件来源都由
// `frame.state.graph` 驱动；播放、倍速、设备分支只改变请求位置，平台端不
// 重算事件顺序，也不修改真实星图。

import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'

interface StarMapProofPoint {
  x: number
  y: number
}

interface StarMapProofNode {
  id: string
  title: string
  position: StarMapProofPoint
}

interface StarMapProofPathSegment {
  type: string
  instanceId?: string
  nodeId?: string
}

interface StarMapProofTarget {
  nodeId?: string
}

interface StarMapProofTargetPath {
  starmapId?: string
  segments?: StarMapProofPathSegment[]
  target?: StarMapProofTarget
}

interface StarMapProofState {
  graph: StarMapProofGraph
}

interface StarMapProofEdge {
  id: string
  from: StarMapProofTargetPath
  to: StarMapProofTargetPath
}

interface StarMapProofLink {
  id: string
  source: StarMapProofTargetPath
  target: StarMapProofTargetPath
}

interface StarMapProofEmbed {
  instanceId: string
  label?: string
  position: StarMapProofPoint
}

interface StarMapProofGraph {
  nodes: StarMapProofNode[]
  edges?: StarMapProofEdge[]
  links?: StarMapProofLink[]
  embeds?: StarMapProofEmbed[]
}

interface StarMapProofEvent {
  actor: string
  entrypoint: string
  operation: string
  objectType: string
  objectId: string
}

interface StarMapProofBranch {
  deviceId: string
  eventCount: number
}

interface StarMapProofFrame {
  starmapId: string
  deviceId: string
  proofFormatVersion: number
  legacyEventCount: number
  position: number
  totalEvents: number
  event: StarMapProofEvent | null
  state: StarMapProofState
  historyComplete: boolean
  valid: boolean
  errors: string[]
  branches: StarMapProofBranch[]
}

@Component
export struct StarMapProofReplayPanel {
  @Prop bridge: IWriterCoreBridge | null = null
  @Prop starmapId: string = ''
  onClose: () => void = () => {}

  @State private frame: StarMapProofFrame | null = null
  @State private playhead: number = 0
  @State private totalEvents: number = 0
  @State private deviceId: string = ''
  @State private playing: boolean = false
  @State private playbackSpeed: number = 1
  @State private errorMessage: string = ''
  @State private canvasWidth: number = 0
  @State private canvasHeight: number = 0
  @State private branchIndex: number = 0
  private timer: number = -1
  private canvasReady: boolean = false
  private settings: RenderingContextSettings = new RenderingContextSettings(true)
  private context: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.settings)
  private readonly speeds: number[] = [0.5, 1, 2, 4, 8]

  aboutToAppear() {
    void this.requestFrame(0)
  }

  aboutToDisappear() {
    this.stopPlayback()
  }

  private async requestFrame(target: number): Promise<void> {
    const bridge = this.bridge
    if (bridge === null || this.starmapId.length === 0) {
      return
    }
    const request = JSON.stringify({
      starmapId: this.starmapId,
      deviceId: this.deviceId,
      targetPosition: Math.max(0, Math.floor(target)),
      includeTrajectory: true
    })
    const envelope = await bridge.replayStarMapProof(request)
    if (!envelope.success || envelope.data === undefined || envelope.data === null) {
      this.errorMessage = envelope.userMessage ?? '无法读取星图创作记录'
      return
    }
    const frame = envelope.data as StarMapProofFrame
    if (this.deviceId.length === 0) {
      this.deviceId = frame.deviceId ?? ''
    }
    this.frame = frame
    this.playhead = frame.position
    this.totalEvents = frame.totalEvents
    this.errorMessage = ''
    this.syncBranchIndex()
    this.draw()
  }

  private syncBranchIndex(): void {
    const branches = this.frame?.branches ?? []
    for (let index = 0; index < branches.length; index++) {
      if (branches[index].deviceId === this.deviceId) {
        this.branchIndex = index
        return
      }
    }
  }

  private branchLabel(): string {
    const branches = this.frame?.branches ?? []
    const branch = branches[this.branchIndex]
    if (branch === undefined) {
      return '设备分支'
    }
    const short = branch.deviceId.length > 12
      ? branch.deviceId.substring(0, 12) : branch.deviceId
    return `${short} · ${branch.eventCount}`
  }

  private cycleBranch(): void {
    const branches = this.frame?.branches ?? []
    if (branches.length <= 1) {
      return
    }
    const next = (this.branchIndex + 1) % branches.length
    this.stopPlayback()
    this.branchIndex = next
    this.deviceId = branches[next].deviceId
    this.playhead = 0
    void this.requestFrame(0)
  }

  private stopPlayback(): void {
    this.playing = false
    if (this.timer >= 0) {
      clearInterval(this.timer)
      this.timer = -1
    }
  }

  private togglePlayback(): void {
    if (this.playing) {
      this.stopPlayback()
      return
    }
    if (this.totalEvents <= 0) {
      return
    }
    if (this.playhead >= this.totalEvents) {
      void this.requestFrame(0)
    }
    this.playing = true
    this.timer = setInterval(() => {
      if (!this.playing) {
        return
      }
      const next = Math.min(this.totalEvents, this.playhead + 1)
      void this.requestFrame(next)
      if (next >= this.totalEvents) {
        this.stopPlayback()
      }
    }, Math.max(16, Math.round(200 / this.playbackSpeed)))
  }

  private cycleSpeed(): void {
    const index = this.speeds.indexOf(this.playbackSpeed)
    this.playbackSpeed = this.speeds[(index + 1) % this.speeds.length]
    if (this.playing) {
      this.stopPlayback()
      this.togglePlayback()
    }
  }

  private sourceText(): string {
    const event = this.frame?.event
    if (event === null || event === undefined) {
      return '起始状态'
    }
    return `${event.actor} · ${event.entrypoint} · ${event.operation} ${event.objectType}`
  }

  private graph(): StarMapProofGraph {
    const graph = this.frame?.state?.graph
    return graph !== undefined && graph !== null
      ? graph
      : { nodes: [], edges: [], links: [], embeds: [] }
  }

  private collectPoints(): Map<string, StarMapProofPoint> {
    const points = new Map<string, StarMapProofPoint>()
    for (const node of this.graph().nodes ?? []) {
      if (node.position !== undefined && node.position !== null) {
        points.set(`node:${node.id}`, node.position)
      }
    }
    for (const embed of this.graph().embeds ?? []) {
      if (embed.position !== undefined && embed.position !== null) {
        points.set(`embed:${embed.instanceId}`, embed.position)
      }
    }
    return points
  }

  /**
   * 端点可能穿过 Embed/Portal 进入子图。优先使用最后一段路径的宿主位置，
   * 这样回放不会把跨层连线直接丢掉。
   */
  private resolveEndpoint(
    path: StarMapProofTargetPath | undefined,
    points: Map<string, StarMapProofPoint>
  ): StarMapProofPoint | null {
    if (path === undefined || path === null) {
      return null
    }
    const segments = path.segments ?? []
    if (segments.length > 0) {
      const last = segments[segments.length - 1]
      if (last.type === 'enterEmbed' && last.instanceId !== undefined) {
        const embed = points.get(`embed:${last.instanceId}`)
        if (embed !== undefined) {
          return embed
        }
      }
      if (last.type === 'enterPortal' && last.nodeId !== undefined) {
        const node = points.get(`node:${last.nodeId}`)
        if (node !== undefined) {
          return node
        }
      }
    }
    const nodeId = path.target?.nodeId
    if (nodeId !== undefined && nodeId.length > 0) {
      return points.get(`node:${nodeId}`) ?? null
    }
    return null
  }

  private draw(): void {
    if (!this.canvasReady || this.canvasWidth <= 0 || this.canvasHeight <= 0) {
      return
    }
    const ctx = this.context
    const width = this.canvasWidth
    const height = this.canvasHeight
    ctx.clearRect(0, 0, width, height)
    ctx.fillStyle = '#F7F8FB'
    ctx.fillRect(0, 0, width, height)

    const graph = this.graph()
    const points = this.collectPoints()
    let minX = 0
    let minY = 0
    let maxX = 1
    let maxY = 1
    let seen = false
    points.forEach((point: StarMapProofPoint) => {
      if (!seen) {
        minX = point.x
        maxX = point.x
        minY = point.y
        maxY = point.y
        seen = true
      } else {
        minX = Math.min(minX, point.x)
        maxX = Math.max(maxX, point.x)
        minY = Math.min(minY, point.y)
        maxY = Math.max(maxY, point.y)
      }
    })
    const spanX = Math.max(1, maxX - minX)
    const spanY = Math.max(1, maxY - minY)
    let scale = Math.min((width - 96) / spanX, (height - 96) / spanY)
    scale = Math.max(0.2, Math.min(scale, 2.2))
    const centerX = (minX + maxX) / 2
    const centerY = (minY + maxY) / 2
    const screenX = (value: number): number => width / 2 + (value - centerX) * scale
    const screenY = (value: number): number => height / 2 + (value - centerY) * scale

    ctx.strokeStyle = '#B8C0CC'
    ctx.lineWidth = 1.5
    for (const edge of graph.edges ?? []) {
      const start = this.resolveEndpoint(edge.from, points)
      const end = this.resolveEndpoint(edge.to, points)
      if (start === null || end === null) {
        continue
      }
      ctx.beginPath()
      ctx.moveTo(screenX(start.x), screenY(start.y))
      ctx.lineTo(screenX(end.x), screenY(end.y))
      ctx.stroke()
    }
    ctx.strokeStyle = '#7C8BE0'
    ctx.setLineDash([5, 4])
    for (const link of graph.links ?? []) {
      const start = this.resolveEndpoint(link.source, points)
      const end = this.resolveEndpoint(link.target, points)
      if (start === null || end === null) {
        continue
      }
      ctx.beginPath()
      ctx.moveTo(screenX(start.x), screenY(start.y))
      ctx.lineTo(screenX(end.x), screenY(end.y))
      ctx.stroke()
    }
    ctx.setLineDash([])

    for (const embed of graph.embeds ?? []) {
      if (embed.position === undefined || embed.position === null) {
        continue
      }
      const x = screenX(embed.position.x)
      const y = screenY(embed.position.y)
      ctx.beginPath()
      ctx.fillStyle = '#EDE9FF'
      ctx.strokeStyle = '#7C8BE0'
      ctx.lineWidth = 2
      ctx.arc(x, y, 18, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      const label = (embed.label ?? embed.instanceId).substring(0, 10)
      ctx.fillStyle = '#5B6170'
      ctx.font = '9px sans-serif'
      ctx.fillText(label, x + 22, y + 3)
    }

    const activeId = this.frame?.event?.objectId ?? ''
    for (const node of graph.nodes ?? []) {
      if (node.position === undefined || node.position === null) {
        continue
      }
      const x = screenX(node.position.x)
      const y = screenY(node.position.y)
      const active = node.id === activeId
      ctx.fillStyle = active ? '#DCE4FF' : '#FFFFFF'
      ctx.strokeStyle = active ? '#4F6DF5' : '#C9CFDA'
      ctx.lineWidth = active ? 2 : 1
      ctx.beginPath()
      ctx.rect(x - 48, y - 18, 96, 36)
      ctx.fill()
      ctx.stroke()
      ctx.fillStyle = '#30343B'
      ctx.font = '11px sans-serif'
      const title = node.title.length > 0 ? node.title : node.id
      ctx.fillText(title.substring(0, 14), x - 40, y + 4)
    }
  }

  build() {
    Column({ space: 8 }) {
      Row() {
        Text('星图创作记录')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
        Blank()
        Button('关闭')
          .fontSize(12)
          .onClick(() => {
            this.stopPlayback()
            this.onClose()
          })
      }
      .width('100%')

      Row({ space: 8 }) {
        Button(this.branchLabel())
          .fontSize(11)
          .enabled((this.frame?.branches ?? []).length > 1)
          .onClick(() => this.cycleBranch())
        Text(`位置 ${this.playhead} / ${this.totalEvents}`)
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .layoutWeight(1)
      }
      .width('100%')

      Text(this.sourceText())
        .fontSize(11)
        .fontColor($r('sys.color.font_tertiary'))
        .width('100%')
        .maxLines(2)

      if (this.frame !== null && !this.frame.historyComplete) {
        Text(this.frame.proofFormatVersion === 1 || this.frame.legacyEventCount > 0
          ? '包含旧格式事件：可回放，但历史与逐字证明不完整'
          : '此前历史不完整（从真实基线开始）')
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
          .width('100%')
      }
      if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(11)
          .fontColor($r('sys.color.warning'))
          .width('100%')
      }

      // 真实画布：节点、连线、Embed 和跨层端点都由 Core 的历史图驱动。
      Canvas(this.context)
        .width('100%')
        .layoutWeight(1)
        .backgroundColor('#F7F8FB')
        .borderRadius(8)
        .onReady(() => {
          this.canvasReady = true
          this.draw()
        })
        .onAreaChange((_oldArea: Area, newArea: Area) => {
          this.canvasWidth = Number(newArea.width)
          this.canvasHeight = Number(newArea.height)
          this.draw()
        })

      Row({ space: 8 }) {
        Button(this.playing ? '暂停' : '播放')
          .fontSize(12)
          .enabled(this.totalEvents > 0)
          .onClick(() => this.togglePlayback())
        Button('上一步')
          .fontSize(12)
          .enabled(this.playhead > 0)
          .onClick(() => {
            this.stopPlayback()
            void this.requestFrame(Math.max(0, this.playhead - 1))
          })
        Button('下一步')
          .fontSize(12)
          .enabled(this.playhead < this.totalEvents)
          .onClick(() => {
            this.stopPlayback()
            void this.requestFrame(Math.min(this.totalEvents, this.playhead + 1))
          })
        Button(`${this.playbackSpeed}x`)
          .fontSize(12)
          .onClick(() => this.cycleSpeed())
      }
      .width('100%')

      Slider({
        value: this.playhead,
        min: 0,
        max: Math.max(1, this.totalEvents),
        step: 1,
        style: SliderStyle.OutSet
      })
        .width('100%')
        .onChange((value: number) => {
          this.stopPlayback()
          const target = Math.max(0, Math.min(this.totalEvents, Math.floor(value)))
          void this.requestFrame(target)
        })
    }
    .width('92%')
    .height('82%')
    .padding(16)
    .backgroundColor($r('sys.color.background_primary'))
    .borderRadius(12)
  }
}
