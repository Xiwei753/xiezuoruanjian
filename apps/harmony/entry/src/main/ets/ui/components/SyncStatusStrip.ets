// SyncStatusStrip — reusable visual summary of the current sync run.
// The caller owns observation, navigation, and sync behavior; this component only renders a snapshot.

import { SyncRunSnapshot } from '../../feature/settings/presentation/SyncRunPresentationState'
import { SyncOutcomePresenter } from '../../feature/settings/presentation/SyncOutcomePresenter'
import { DesignTokens } from '../theme/DesignTokens'

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

  private isConflict(): boolean {
    const outcome = this.snapshot.outcome
    if (outcome === null) {
      return false
    }
    return outcome.errorCode === 'SYNC_CONFLICT' ||
      outcome.messageKey === 'sync.result.conflict_summary' ||
      outcome.overallStatus === 'conflict' ||
      outcome.overallStatus === 'partial_conflict' ||
      (outcome.totalConflicts !== undefined && outcome.totalConflicts > 0)
  }

  private statusTitle(): string {
    if (this.snapshot.phase === 'in_progress') {
      return this.snapshot.source === 'auto' ? '正在自动同步…' : '正在同步…'
    }
    if (this.snapshot.phase === 'success') {
      return '同步完成'
    }
    return this.isConflict() ? '同步冲突' : '同步失败'
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
    const summary: string = SyncOutcomePresenter.present(context, outcome)
    if (!this.isConflict() || outcome.totalConflicts === undefined) {
      return summary
    }
    const countFormat: string = context.resourceManager
      .getStringSync($r('app.string.sync_conflicts_count_format').id)
    return `${summary} · ${countFormat.replace('%s', outcome.totalConflicts.toString())}`
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
            .width(20)
            .height(20)
        } else if (this.snapshot.phase === 'success') {
          Text('✓')
            .fontSize(18)
            .fontColor($r('sys.color.font_primary'))
        } else {
          Text('!')
            .fontSize(18)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.ohos_id_color_warning'))
        }

        Column() {
          Text(this.statusTitle())
            .fontSize(14)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))

          if (this.snapshot.phase === 'success' && this.successSummary().length > 0) {
            Text(this.successSummary())
              .fontSize(12)
              .fontColor($r('sys.color.font_secondary'))
              .margin({ top: 2 })
          } else if (this.snapshot.phase === 'failed') {
            Text(this.failureSummary())
              .fontSize(12)
              .fontColor($r('sys.color.font_secondary'))
              .margin({ top: 2 })
          }
        }
        .layoutWeight(1)
        .margin({ left: 10 })
        .alignItems(HorizontalAlign.Start)

        if (this.snapshot.phase === 'failed') {
          Button('查看详情')
            .fontSize(12)
            .height(32)
            .onClick(() => {
              this.onOpenDetails()
            })
        }
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 8, bottom: 8 })
      .alignItems(VerticalAlign.Center)
      .backgroundColor($r('sys.color.background_secondary'))
      .borderRadius(DesignTokens.cardRadius)
    }
  }
}
