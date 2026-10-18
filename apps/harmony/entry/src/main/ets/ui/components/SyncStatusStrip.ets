// SyncStatusStrip — reusable visual summary of the current sync run.
// The caller owns observation, navigation, and sync behavior; this component only renders a snapshot.

import { SyncRunSnapshot } from '../../feature/settings/presentation/SyncRunPresentationState'
import { SyncOutcome } from '../../feature/settings/presentation/SyncRunTypes'
import { SyncOutcomePresenter } from '../../feature/settings/presentation/SyncOutcomePresenter'
import { DesignTokens } from '../theme/DesignTokens'

/** Shared conflict classification for the strip and its shell host. */
export function isSyncOutcomeConflict(outcome: SyncOutcome | null): boolean {
  if (outcome === null) {
    return false
  }
  if (outcome.errorCode === 'SYNC_SESSION_UNALIGNED' ||
    outcome.messageKey === 'sync.result.session_unaligned') {
    return false
  }
  return outcome.errorCode === 'SYNC_CONFLICT' ||
    outcome.messageKey === 'sync.result.conflict_summary' ||
    outcome.overallStatus === 'conflict' ||
    outcome.overallStatus === 'partial_conflict' ||
    (outcome.totalConflicts !== undefined && outcome.totalConflicts > 0)
}

@ComponentV2
export struct SyncStatusStrip {
  @Param snapshot: SyncRunSnapshot = {
    phase: 'idle',
    source: 'manual',
    startedAtMs: null,
    finishedAtMs: null,
    outcome: null
  }
  /** Failure/conflict detail navigation is supplied by the host page. */
  @Param onOpenDetails: () => void = () => {}
  /** Failed runs stay available as a small entry until the user dismisses it. */
  @Param onDismiss: () => void = () => {}

  private isConflict(): boolean {
    return isSyncOutcomeConflict(this.snapshot.outcome)
  }

  private statusTitle(): string {
    if (this.snapshot.phase === 'in_progress') {
      return this.snapshot.source === 'auto' ? '正在自动同步…' : '正在同步…'
    }
    if (this.snapshot.outcome?.errorCode === 'SYNC_SESSION_UNALIGNED' ||
      this.snapshot.outcome?.messageKey === 'sync.result.session_unaligned') {
      return '写作会话未对齐'
    }
    if (this.isConflict()) {
      return '同步冲突'
    }
    return this.snapshot.phase === 'success' ? '同步完成' : '同步失败'
  }

  private successSummary(): string {
    const outcome = this.snapshot.outcome
    if (outcome === null) {
      return ''
    }
    if (outcome.overallStatus === 'no_changes') {
      return '没有文件变化'
    }

    const parts: string[] = []
    if (outcome.totalDownloaded !== undefined) {
      parts.push(`已下载 ${outcome.totalDownloaded} 个文件`)
    }
    if (outcome.totalUploaded !== undefined) {
      parts.push(`已上传 ${outcome.totalUploaded} 个文件`)
    }
    return parts.join(' · ')
  }

  private failureSummary(): string {
    const outcome = this.snapshot.outcome
    if (outcome === null) {
      return '同步失败'
    }

    const context = getContext(this)
    return SyncOutcomePresenter.present(context, outcome)
  }

  build() {
    if (this.snapshot.phase === 'idle') {
      Blank()
        .width(0)
        .height(0)
    } else {
      Row() {
        if (this.snapshot.phase === 'in_progress') {
          LoadingProgress()
            .width(18)
            .height(18)
        } else if (this.isConflict()) {
          Text('!')
            .fontSize(16)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.ohos_id_color_warning'))
        } else if (this.snapshot.phase === 'success') {
          Text('✓')
            .fontSize(16)
            .fontColor($r('sys.color.font_primary'))
        } else {
          Text('!')
            .fontSize(16)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.ohos_id_color_warning'))
        }

        Column() {
          Text(this.statusTitle())
            .fontSize(13)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))

          if (this.isConflict()) {
            Text(this.failureSummary())
              .fontSize(11)
              .fontColor($r('sys.color.font_secondary'))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .margin({ top: 1 })
          } else if (this.snapshot.phase === 'success' && this.successSummary().length > 0) {
            Text(this.successSummary())
              .fontSize(11)
              .fontColor($r('sys.color.font_secondary'))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .margin({ top: 1 })
          } else if (this.snapshot.phase === 'failed') {
            Text(this.failureSummary())
              .fontSize(11)
              .fontColor($r('sys.color.font_secondary'))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .margin({ top: 1 })
          }
        }
        .layoutWeight(1)
        .margin({ left: 8 })
        .alignItems(HorizontalAlign.Start)

        if (this.snapshot.phase === 'failed' || this.isConflict()) {
          Button('详情')
            .fontSize(11)
            .height(30)
            .padding({ left: 8, right: 8 })
            .accessibilityText('查看同步详情')
            .onClick(() => {
              this.onOpenDetails()
            })
          Button('×')
            .fontSize(17)
            .width(30)
            .height(30)
            .padding(0)
            .backgroundColor(Color.Transparent)
            .accessibilityText('关闭同步提示')
            .onClick(() => {
              this.onDismiss()
            })
        }
      }
      .width('100%')
      .padding({ left: 10, right: 8, top: 6, bottom: 6 })
      .alignItems(VerticalAlign.Center)
      .backgroundColor($r('sys.color.background_secondary'))
      .borderRadius(DesignTokens.cardRadius)
    }
  }
}
