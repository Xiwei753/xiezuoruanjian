// StatsScreen — 统计页 UI。
// 数据来自 Core 的 NativeStatsBridge（getWritingStats），仅做 UI 展示，
// 不复制业务状态机（遵守根 AGENTS.md 平台端边界）。

import hilog from '@ohos.hilog'
import { NativeStatsBridge } from '../../../corebridge/native/NativeStatsBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { HarmonyThemeAdapter, ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole, WidthClass } from '../../../corebridge/dto/PlatformDtos'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'

@Component
export struct StatsScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State stats: WritingStats | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()

  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    hilog.info(0x0000, 'StatsScreen', 'route name=Stats')
    this.loadStats()

    const self = this
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()
  }

  aboutToDisappear() {
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  async loadStats() {
    this.isLoading = true
    this.errorMessage = ''
    try {
      const bridge = new NativeStatsBridge(new NativeCoreModule())
      const result: ResultEnvelope<WritingStats> = await bridge.getWritingStats()
      if (result.success && result.data) {
        this.stats = result.data
      } else {
        // envelope 只有 errorCode / userMessage（Core 不发明 messageKey）。
        this.errorMessage = result.userMessage || result.errorCode || '加载统计失败'
      }
    } catch (err) {
      this.errorMessage = `加载统计失败: ${err}`
    } finally {
      this.isLoading = false
    }
  }

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    // StatsScreen 自带 header，hideTitleBar(true) 隐藏原生标题栏。
    NavDestination() {
    Column() {
      Row() {
        if (this.adaptiveState.sizeClass !== WidthClass.Compact) {
          Image($r('sys.media.ohos_ic_public_arrow_left'))
            .width(24)
            .height(24)
            .onClick(() => this.navPathStack.pop())
        }

        Text('统计')
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .margin({ left: 12 })

        Blank()
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 16, bottom: 12 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
          Text('加载中...')
            .fontSize(14)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.loadStats())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.stats) {
        Scroll() {
          Column() {
            // Issue #753 评论 5814008597 问题2：WritingStats 已换成 Core 的 WritingStatsSummaryDto（最近 30 天汇总）。
            // 旧字段 totalChars/totalWords/totalChapters/todayChars/todayDuration/averageSpeed 全部不存在，
            // 改为展示新 DTO 的全部字段，并在首行展示统计范围。
            Row() {
              Text('统计范围')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.range.startDate} ~ ${this.stats.range.endDate}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('手写字数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalHumanTypedChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('粘贴字数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalPastedChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('删除字数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalDeletedChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('AI插入字数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalAiInsertedChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('净增字数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalNetDeltaChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('活跃时长(秒)')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalActiveSeconds}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('写作会话数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.totalSessions}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('写作天数')
                .fontSize(16)
                .layoutWeight(1)
              Text(`${this.stats.daysCount}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })
          }
          .width('100%')
          .padding({ left: 16, right: 16, bottom: 24 })
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
        .align(Alignment.Top)
      }
    }
    .width('100%')
    .height('100%')
    }
    .hideTitleBar(true)
    .onShown(() => {
      NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Stats, 2)
    })
  }
}
