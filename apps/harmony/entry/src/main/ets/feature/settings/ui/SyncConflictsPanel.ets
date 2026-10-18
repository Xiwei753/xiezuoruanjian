// ─── SyncConflictsPanel.ets ───
// 同步冲突面板（Issue #926）：列出 Core 全局冲突列表，逐项预览双方内容并解决。
//
// - 数据与操作都在 SyncConflictViewModel：列表来自 NativeSyncBridge.listAllSyncConflicts()，
//   解决后重新从 Core 拉取剩余冲突；面板不维护第二份冲突列表。
// - "合并"是真实合并流程：以本地版本（或本地为空时的远端版本）为草稿人工编辑，
//   保存时先真实写回章节、Core 确认成功后才清除冲突标记（见 viewModel.saveMerged）。
// - 面板挂在窗口层级（openCustomDialog + ComponentContent），与 SettingsOverlayPanel /
//   CreateTitleDialog 同一套写法：由系统负责遮罩、居中与返回拦截。
// - "重新同步"只走 SyncOperationController.performSync()：#923 的 withSyncBarrier 会在
//   同步前保存活跃正文、同步后按落盘身份回填打开中的章节，面板不复制编辑器会话逻辑。

import hilog from '@ohos.hilog'
import { UIContext, ComponentContent } from '@kit.ArkUI'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { SyncConflictViewModel, SyncConflictItemState, SyncConflictDecision } from '../presentation/SyncConflictViewModel'
import { SyncOperationController } from '../presentation/SyncOperationController'
import { SyncOutcome } from '../presentation/SyncRunTypes'
import { SyncOutcomePresenter } from '../presentation/SyncOutcomePresenter'

const TAG = 'SyncConflictsPanel'
const DOMAIN = 0x0000

/** 预览正文展示上限：只截断显示，不改写 Core 返回的数据。 */
const MAX_PREVIEW_CHARS: number = 1200

/** 待展示的预览文本（truncated 表示仅显示开头部分）。 */
interface PreviewText {
  text: string
  truncated: boolean
}

/** 按 targetKind / projectTitle 聚合的冲突分组。 */
interface SyncConflictGroup {
  key: string
  title: string
  itemCount: number
  items: SyncConflictItemState[]
}

/** 兼容 `string` / `string | null` 两种 DTO 字段类型；只用于展示。 */
function safeDisplayText(value: string | null): string {
  return value === null ? '' : value
}

/** 判定"远端内容缺失"时兼容两种 DTO 字段类型。 */
function isNullText(value: string | null): boolean {
  return value === null
}

/** 截断长文本用于展示；不改写原数据。 */
function truncatePreview(raw: string): PreviewText {
  if (raw.length <= MAX_PREVIEW_CHARS) {
    return { text: raw, truncated: false }
  }
  return { text: `${raw.substring(0, MAX_PREVIEW_CHARS)}…`, truncated: true }
}

/**
 * 冲突面板参数。
 * onRemainingChanged：每次从 Core 重新拉取列表后回传剩余冲突数，
 * 设置页据此更新"查看冲突（N 处）"入口计数。
 */
export interface SyncConflictsPanelArgs {
  onRequestClose: () => void
  onRemainingChanged: (remaining: number) => void
}

@ComponentV2
export struct SyncConflictsPanel {
  @Param onRequestClose: () => void = () => {}
  @Param onRemainingChanged: (remaining: number) => void = (_remaining: number) => {}
  @Local viewModel: SyncConflictViewModel = new SyncConflictViewModel()
  @Local reSyncing: boolean = false
  @Local reSyncMessage: string = ''
  // 本轮面板里是否成功解决过冲突（决定是否提示重新同步）。
  @Local resolvedAny: boolean = false

  aboutToAppear(): void {
    void this.reloadAndReport()
  }

  build() {
    Row() {
      Column() {
        this.PanelHeader()

        if (this.viewModel.isLoading && this.viewModel.items.length === 0) {
          this.LoadingBlock()
        } else if (this.viewModel.listErrorCode.length > 0 && this.viewModel.items.length === 0) {
          this.ListErrorBlock()
        } else if (this.viewModel.items.length === 0) {
          this.EmptyBlock()
        } else {
          // 刷新失败但还有上一次列表：显示结构化 errorCode，列表继续可用。
          if (this.viewModel.listErrorCode.length > 0) {
            Text(this.listFailedLabel())
              .fontSize(12)
              .margin({ bottom: 8 })
          }
          if (this.resolvedAny) {
            Text($r('app.string.sync_conflicts_resolved_hint'))
              .fontSize(12)
              .margin({ bottom: 8 })
          }
          this.ConflictList()
        }

        this.PanelFooter()
      }
      .width('100%')
      .height('100%')
      .constraintSize({ maxWidth: 960 })
      .padding(20)
      .backgroundColor($r('sys.color.background_secondary'))
      .borderRadius(DesignTokens.dialogRadius)
    }
    .width('100%')
    .height('84%')
    .padding({ left: 24, right: 24 })
    .justifyContent(FlexAlign.Center)
  }

  // ── 面板头：标题 + 计数 + 刷新 + 关闭 ──
  @Builder
  PanelHeader() {
    Row() {
      Text($r('app.string.sync_conflicts_title'))
        .fontSize(20)
        .fontWeight(FontWeight.Bold)
      if (this.viewModel.items.length > 0) {
        Text(this.countLabel(this.viewModel.items.length))
          .fontSize(12)
          .margin({ left: 8 })
      }

      Blank()

      Button($r('app.string.sync_conflicts_refresh'), { type: ButtonType.Normal })
        .height(32)
        .fontSize(13)
        .fontColor($r('sys.color.font_primary'))
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius(DesignTokens.radiusSm)
        .enabled(!this.viewModel.isBusy() && !this.reSyncing)
        .onClick(() => { void this.reloadAndReport() })

      Button($r('app.string.sync_conflicts_close'), { type: ButtonType.Normal })
        .height(32)
        .fontSize(13)
        .fontColor($r('sys.color.font_primary'))
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius(DesignTokens.radiusSm)
        .margin({ left: 8 })
        .onClick(() => { this.onRequestClose() })
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
    .margin({ bottom: 12 })
  }

  // ── 加载中 ──
  @Builder
  LoadingBlock() {
    Column({ space: 8 }) {
      LoadingProgress()
        .width(32)
        .height(32)
      Text($r('app.string.loading'))
        .fontSize(13)
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 列表加载失败（无可用列表） ──
  @Builder
  ListErrorBlock() {
    Column({ space: 12 }) {
      Text(this.listFailedLabel())
        .fontSize(14)
      Button($r('app.string.retry'), { type: ButtonType.Normal })
        .height(32)
        .fontSize(13)
        .onClick(() => { void this.reloadAndReport() })
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 无冲突 ──
  @Builder
  EmptyBlock() {
    Column({ space: 8 }) {
      Text($r('app.string.sync_conflicts_empty'))
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
      Text($r('app.string.sync_conflicts_empty_hint'))
        .fontSize(13)
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  // ── 冲突列表（按 target/作品分组） ──
  @Builder
  ConflictList() {
    Scroll() {
      Column() {
        ForEach(this.buildGroups(), (group: SyncConflictGroup) => {
          Column() {
            Row() {
              Text(group.title)
                .fontSize(14)
                .fontWeight(FontWeight.Medium)
                .layoutWeight(1)
              Text(this.countLabel(group.itemCount))
                .fontSize(12)
            }
            .width('100%')
            .margin({ bottom: 8 })

            ForEach(group.items, (item: SyncConflictItemState) => {
              this.ConflictItemCard(item)
            }, (item: SyncConflictItemState) => item.key())
          }
          .width('100%')
          .margin({ bottom: 16 })
        }, (group: SyncConflictGroup) => group.key)
      }
      .width('100%')
      .padding({ bottom: 8 })
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .align(Alignment.Top)
  }

  // ── 单条冲突卡片 ──
  @Builder
  ConflictItemCard(item: SyncConflictItemState) {
    Column() {
      // 概要行：路径 / 类型 / 创建时间；整行点击展开。
      Row() {
        Column({ space: 4 }) {
          Text(item.path)
            .fontSize(14)
            .width('100%')
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Row({ space: 12 }) {
            Text(this.kindLabel(item.kind))
              .fontSize(12)
            Text(this.formatTime(item.createdAt))
              .fontSize(12)
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Image($r('sys.media.ohos_ic_public_arrow_down'))
          .width(18)
          .height(18)
          .rotate({ angle: item.expanded ? 180 : 0 })
          .animation({ duration: 150, curve: Curve.EaseInOut })
      }
      .width('100%')
      .onClick(() => { this.viewModel.toggleExpand(item) })

      if (item.expanded) {
        Column() {
          if (item.previewLoading) {
            Row({ space: 8 }) {
              LoadingProgress()
                .width(18)
                .height(18)
              Text($r('app.string.loading'))
                .fontSize(13)
            }
            .width('100%')
          } else if (item.previewErrorCode.length > 0) {
            Row({ space: 8 }) {
              Text(this.previewFailedLabel(item.previewErrorCode))
                .fontSize(12)
                .layoutWeight(1)
              Button($r('app.string.retry'), { type: ButtonType.Normal })
                .height(28)
                .fontSize(12)
                .onClick(() => { void this.viewModel.loadPreview(item) })
            }
            .width('100%')
          } else if (item.preview !== null) {
            // 双方内容（长文本仅截断展示，不改写数据）。
            this.ContentBlock($r('app.string.sync_conflicts_local_content'), this.localPreviewText(item))
            this.ContentBlock($r('app.string.sync_conflicts_remote_content'), this.remotePreviewText(item))
          }

          // 操作：保留本地 / 采用远端 / 合并（合并先进入编辑，保存时真实写回章节）。
          if (!item.mergeEditing) {
            Row({ space: 8 }) {
              if (item.resolving) {
                LoadingProgress()
                  .width(18)
                  .height(18)
              }
              Button($r('app.string.sync_conflicts_keep_local'), { type: ButtonType.Normal })
                .layoutWeight(1)
                .height(32)
                .fontSize(13)
                .enabled(!this.viewModel.isBusy() && !this.reSyncing)
                .onClick(() => { void this.handleDecision(item, 'keep_local') })
              Button($r('app.string.sync_conflicts_take_remote'), { type: ButtonType.Normal })
                .layoutWeight(1)
                .height(32)
                .fontSize(13)
                .enabled(!this.viewModel.isBusy() && !this.reSyncing)
                .onClick(() => { void this.handleDecision(item, 'take_remote') })
              Button($r('app.string.sync_conflicts_merge_open'), { type: ButtonType.Normal })
                .layoutWeight(1)
                .height(32)
                .fontSize(13)
                .enabled(!this.viewModel.isBusy() && !this.reSyncing && item.canMergeInPanel())
                .onClick(() => { this.viewModel.beginMerge(item) })
            }
            .width('100%')
            .margin({ top: 8 })

            // 不支持合并/缺内容时说明原因。
            if (item.previewLoaded && !item.canMergeInPanel()) {
              Text(this.mergeDisabledReason(item))
                .fontSize(12)
                .margin({ top: 6 })
            }
          } else {
            // 合并编辑：草稿保存为合并结果，Core 确认写入后才清除冲突标记。
            this.MergeEditor(item)
          }

          // 解决失败：展示 Core 返回的结构化 errorCode。
          if (item.actionErrorCode.length > 0) {
            Text(this.actionFailedLabel(item.actionErrorCode))
              .fontSize(12)
              .margin({ top: 6 })
          }
        }
        .width('100%')
        .margin({ top: 12 })
      }
    }
    .width('100%')
    .padding(12)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .margin({ bottom: 8 })
  }

  @Builder
  ContentBlock(label: ResourceStr, content: PreviewText) {
    Column({ space: 4 }) {
      Text(label)
        .fontSize(12)
        .fontWeight(FontWeight.Medium)
      Text(content.text)
        .fontSize(13)
        .width('100%')
      if (content.truncated) {
        Text($r('app.string.sync_conflicts_truncated_note'))
          .fontSize(11)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(8)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.background_secondary'))
    .margin({ bottom: 8 })
  }

  /**
   * 合并编辑器：以本地版本（或本地为空时的远端版本）为草稿，
   * 保存后作为合并结果写回章节；面板内不做冲突算法，只提供人工编辑。
   */
  @Builder
  MergeEditor(item: SyncConflictItemState) {
    Column({ space: 8 }) {
      Text($r('app.string.sync_conflicts_merge_hint'))
        .fontSize(12)
        .width('100%')
      TextArea({ text: item.mergeDraft })
        .width('100%')
        .height(160)
        .fontSize(13)
        .enabled(!this.viewModel.isBusy() && !this.reSyncing)
        .onChange((value: string) => {
          item.mergeDraft = value
        })
      Row({ space: 8 }) {
        Button($r('app.string.sync_conflicts_merge_save'), { type: ButtonType.Normal })
          .layoutWeight(1)
          .height(32)
          .fontSize(13)
          .enabled(!this.viewModel.isBusy() && !this.reSyncing)
          .onClick(() => { void this.handleMergeSave(item) })
        Button($r('app.string.sync_conflicts_merge_cancel'), { type: ButtonType.Normal })
          .layoutWeight(1)
          .height(32)
          .fontSize(13)
          .fontColor($r('sys.color.font_primary'))
          .backgroundColor($r('sys.color.comp_background_gray'))
          .enabled(!this.viewModel.isBusy())
          .onClick(() => { this.viewModel.cancelMerge(item) })
      }
      .width('100%')
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(8)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor($r('sys.color.background_secondary'))
    .margin({ top: 8 })
  }

  // ── 面板底：重新同步 + 关闭 ──
  @Builder
  PanelFooter() {
    Column({ space: 8 }) {
      if (this.viewModel.resolveNoticeCode.length > 0) {
        Text(this.actionFailedLabel(this.viewModel.resolveNoticeCode))
          .fontSize(12)
      }
      if (this.reSyncMessage.length > 0) {
        Text(this.reSyncMessage)
          .fontSize(12)
      }
      Row({ space: 12 }) {
        Button($r('app.string.sync_conflicts_resync'), { type: ButtonType.Normal })
          .layoutWeight(1)
          .height(36)
          .fontSize(14)
          .fontColor($r('sys.color.font_primary'))
          .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
          .borderRadius(DesignTokens.radiusSm)
          .enabled(!this.reSyncing && !this.viewModel.isBusy())
          .onClick(() => { void this.runReSync() })
        Button($r('app.string.sync_conflicts_close'), { type: ButtonType.Normal })
          .layoutWeight(1)
          .height(36)
          .fontSize(14)
          .fontColor($r('sys.color.font_primary'))
          .backgroundColor($r('sys.color.comp_background_gray'))
          .borderRadius(DesignTokens.radiusSm)
          .onClick(() => { this.onRequestClose() })
      }
      .width('100%')
    }
    .width('100%')
    .margin({ top: 12 })
  }

  // ── 数据/操作 ──

  private async reloadAndReport(): Promise<void> {
    await this.viewModel.loadConflicts()
    this.reportRemaining()
  }

  private async handleDecision(item: SyncConflictItemState, decision: SyncConflictDecision): Promise<void> {
    const resolved: boolean = await this.viewModel.resolve(item, decision)
    if (resolved) {
      this.resolvedAny = true
      this.reportRemaining()
    }
  }

  /** 保存合并结果：真实写回章节 → Core 清除冲突标记 → 刷新剩余冲突。 */
  private async handleMergeSave(item: SyncConflictItemState): Promise<void> {
    const merged: boolean = await this.viewModel.saveMerged(item)
    if (merged) {
      this.resolvedAny = true
      this.reportRemaining()
    }
  }

  // 列表拉取失败时不回传计数：外层入口继续按同步结果展示，不把未知当成"0 处冲突"。
  private reportRemaining(): void {
    if (this.viewModel.listErrorCode.length === 0) {
      this.onRemainingChanged(this.viewModel.items.length)
    }
  }

  /**
   * 重新同步：只走 #923 已建立的唯一同步事务入口（含活跃写作会话保存/回填），
   * 不在面板里复制第二套同步链路。
   */
  private async runReSync(): Promise<void> {
    if (this.reSyncing) {
      return
    }
    this.reSyncing = true
    this.reSyncMessage = ''
    try {
      const outcome: SyncOutcome = await SyncOperationController.getInstance().performSync()
      this.reSyncMessage = SyncOutcomePresenter.present(getContext(this), outcome)
    } catch (e) {
      // 控制器内部已把异常收敛为 SyncOutcome；这里是最后兜底，只记异常类型。
      hilog.error(DOMAIN, TAG, 'runReSync exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      this.reSyncMessage = this.str($r('app.string.error_internal'))
    } finally {
      this.reSyncing = false
    }
    await this.reloadAndReport()
  }

  // ── 派生数据与文案 ──

  /** 按 targetKind / projectTitle 分组；分组顺序与 Core 列表顺序一致。 */
  private buildGroups(): SyncConflictGroup[] {
    const groups: SyncConflictGroup[] = []
    for (const item of this.viewModel.items) {
      const key: string = item.targetKind === 'app' ? 'app' : `project:${item.projectId}`
      let current: SyncConflictGroup | null = null
      for (const group of groups) {
        if (group.key === key) {
          current = group
          break
        }
      }
      if (current === null) {
        const created: SyncConflictGroup = {
          key: key,
          title: this.groupTitle(item),
          itemCount: 0,
          items: []
        }
        current = created
        groups.push(created)
      }
      current.items.push(item)
      current.itemCount = current.items.length
    }
    return groups
  }

  private groupTitle(item: SyncConflictItemState): string {
    if (item.targetKind === 'app') {
      return this.str($r('app.string.sync_conflicts_group_app'))
    }
    if (item.projectTitle.length > 0) {
      return item.projectTitle
    }
    return item.projectId.length > 0
      ? item.projectId
      : this.str($r('app.string.sync_conflicts_unknown_project'))
  }

  private kindLabel(kind: string): string {
    if (kind === 'both_changed') {
      return this.str($r('app.string.sync_conflicts_kind_both_changed'))
    }
    if (kind === 'remote_deleted') {
      return this.str($r('app.string.sync_conflicts_kind_remote_deleted'))
    }
    return kind
  }

  private localPreviewText(item: SyncConflictItemState): PreviewText {
    if (item.preview === null) {
      return { text: '', truncated: false }
    }
    const raw: string = safeDisplayText(item.preview.localContent)
    if (raw.length === 0) {
      return { text: this.str($r('app.string.sync_conflicts_content_empty')), truncated: false }
    }
    return truncatePreview(raw)
  }

  private remotePreviewText(item: SyncConflictItemState): PreviewText {
    if (item.preview === null) {
      return { text: '', truncated: false }
    }
    if (item.preview.remoteDeleted) {
      return { text: this.str($r('app.string.sync_conflicts_remote_deleted_note')), truncated: false }
    }
    if (isNullText(item.preview.remoteContent)) {
      return { text: this.str($r('app.string.sync_conflicts_no_remote_content')), truncated: false }
    }
    const raw: string = safeDisplayText(item.preview.remoteContent)
    if (raw.length === 0) {
      return { text: this.str($r('app.string.sync_conflicts_content_empty')), truncated: false }
    }
    return truncatePreview(raw)
  }

  private mergeDisabledReason(item: SyncConflictItemState): string {
    if (item.preview !== null && item.preview.remoteDeleted) {
      return this.str($r('app.string.sync_conflicts_merge_disabled_remote_deleted'))
    }
    if (item.resolveChapterId() === null) {
      return this.str($r('app.string.sync_conflicts_merge_disabled_unsupported'))
    }
    return this.str($r('app.string.sync_conflicts_merge_disabled_missing'))
  }

  private countLabel(count: number): string {
    return this.str($r('app.string.sync_conflicts_count_format')).replace('%s', count.toString())
  }

  private previewFailedLabel(errorCode: string): string {
    return this.str($r('app.string.sync_conflicts_preview_failed_format')).replace('%s', errorCode)
  }

  private actionFailedLabel(errorCode: string): string {
    return this.str($r('app.string.sync_conflicts_action_failed_format')).replace('%s', errorCode)
  }

  private listFailedLabel(): string {
    return this.str($r('app.string.sync_conflicts_list_failed_format'))
      .replace('%s', this.viewModel.listErrorCode)
  }

  /** Core SyncConflict.created_at 为 Unix 秒（chrono timestamp()），转毫秒后再格式化。 */
  private formatTime(seconds: number): string {
    if (seconds <= 0) {
      return '—'
    }
    const date = new Date(seconds * 1000)
    return `${date.getFullYear()}-${this.twoDigits(date.getMonth() + 1)}-${this.twoDigits(date.getDate())} ` +
      `${this.twoDigits(date.getHours())}:${this.twoDigits(date.getMinutes())}`
  }

  private twoDigits(value: number): string {
    return value < 10 ? `0${value}` : `${value}`
  }

  private str(res: Resource): string {
    return getContext(this).resourceManager.getStringSync(res.id)
  }
}

/** 文件级 @Builder 函数：把 ComponentContent 传来的参数转交给组件。 */
@Builder
function SyncConflictsPanelBuilder(args: SyncConflictsPanelArgs) {
  SyncConflictsPanel({
    onRequestClose: args.onRequestClose,
    onRemainingChanged: args.onRemainingChanged
  })
}

/**
 * 构造同步冲突面板的 ComponentContent。
 * 调用方用 getUIContext().getPromptAction().openCustomDialog(content, {...}) 打开，
 * 关闭时用同一个 content 调用 closeCustomDialog。
 *
 * @param uiContext 调用方的 UIContext
 * @param args 面板参数
 * @returns 可用于 openCustomDialog / closeCustomDialog 的 ComponentContent
 */
export function createSyncConflictsPanelContent(
  uiContext: UIContext,
  args: SyncConflictsPanelArgs
): ComponentContent<SyncConflictsPanelArgs> {
  return new ComponentContent<SyncConflictsPanelArgs>(uiContext, wrapBuilder(SyncConflictsPanelBuilder), args)
}
