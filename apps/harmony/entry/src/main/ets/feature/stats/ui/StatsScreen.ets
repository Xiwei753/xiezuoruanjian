// StatsScreen — 统计页 UI。
// 数据来自 Core 的 NativeStatsBridge（getWritingStats），仅做 UI 展示，
// 不复制业务状态机（遵守根 AGENTS.md 平台端边界）。

import { NativeStatsBridge } from '../../../corebridge/native/NativeStatsBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { WritingStats } from '../../../corebridge/dto/ProjectDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { HarmonyThemeAdapter, ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'

@Component
export struct StatsScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State stats: WritingStats | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()

  aboutToAppear() {
    this.loadStats()
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
        this.errorMessage = result.messageKey || result.errorCode || '加载统计失败'
      }
    } catch (err) {
      this.errorMessage = `加载统计失败: ${err}`
    } finally {
      this.isLoading = false
    }
  }

  build() {
    Column() {
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => this.navPathStack.pop())

        Text('统计')
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
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
            .color(this.themeConfig.colors.primary)
          Text('加载中...')
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.loadStats())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.stats) {
        Scroll() {
          Column() {
            Row() {
              Text('总字数')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.totalChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('总词数')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.totalWords}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('总章节')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.totalChapters}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('今日字数')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.todayChars}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('今日时长(秒)')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.todayDuration}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
            }
            .width('100%')
            .padding({ top: 12, bottom: 12 })

            Row() {
              Text('平均速度')
                .fontSize(16)
                .fontColor(this.themeConfig.colors.textPrimary)
                .layoutWeight(1)
              Text(`${this.stats.averageSpeed}`)
                .fontSize(16)
                .fontWeight(FontWeight.Medium)
                .fontColor(this.themeConfig.colors.textPrimary)
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
    .backgroundColor(this.themeConfig.colors.background)
  }
}
