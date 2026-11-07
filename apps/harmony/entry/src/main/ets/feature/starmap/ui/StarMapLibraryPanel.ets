// StarMapLibraryPanel — 星图素材库（Issue #1037）。
//
// 只展示一个作用域的库目录：全局库或当前作品库。条目是「库归属」，
// 单击打开原素材（星图进入原画布，节点定位引用），拖入/菜单插入才创建
// 当前画布引用；删除库条目不会删除共享本体。

import { StarMapAssetReference, StarMapAssetRef, StarMapLibraryEntryView, StarMapLibraryFolder, StarMapLibraryView } from '../../../corebridge/dto/StarMapDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { getBridge } from '../../../app/AppContext'
import { CompactTextDialogArgs, openCompactTextDialog } from '../../../ui/components/CompactDialogs'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

@ComponentV2
export struct StarMapLibraryPanel {
  @Param @Watch('onScopeChanged') scopeJson: string = '{"kind":"global"}'
  @Param scopeLabel: string = '全局素材库'
  @Param otherScopeJson: string = ''
  @Param canInsert: boolean = true
  @Param onInsert: (target: StarMapAssetRef, title: string) => void =
    (_target: StarMapAssetRef, _title: string): void => {}
  @Param onOpen: (target: StarMapAssetRef, title: string) => void =
    (_target: StarMapAssetRef, _title: string): void => {}
  @Param onReferenceJump: (reference: StarMapAssetReference) => void =
    (_reference: StarMapAssetReference): void => {}

  @Local entries: StarMapLibraryEntryView[] = []
  @Local folders: StarMapLibraryFolder[] = []
  @Local errorMessage: string = ''
  @Local loading: boolean = false
  @Local selectedFolderId: string = ''
  @Local menuEntry: StarMapLibraryEntryView | null = null
  @Local referencesSheetVisible: boolean = false
  @Local references: StarMapAssetReference[] = []
  @Local referencesTitle: string = ''

  private bridge: IWriterCoreBridge = getBridge()

  aboutToAppear(): void {
    this.reload()
  }

  private onScopeChanged(): void {
    this.reload()
  }

  private async reload(): Promise<void> {
    if (this.scopeJson.length === 0) {
      this.entries = []
      this.folders = []
      return
    }
    this.loading = true
    this.errorMessage = ''
    try {
      const result = await this.bridge.listStarMapLibrary(this.scopeJson)
      if (result.success && result.data !== undefined) {
        const view: StarMapLibraryView = result.data
        this.entries = view.entries
        this.folders = view.folders
      } else {
        this.errorMessage = result.userMessage ?? '加载素材库失败'
      }
    } catch (e) {
      this.errorMessage = '加载素材库失败'
    } finally {
      this.loading = false
    }
  }

  private folderName(folderId: string | null): string {
    if (folderId === null) {
      return ''
    }
    for (const folder of this.folders) {
      if (folder.folderId === folderId) {
        return folder.name
      }
    }
    return ''
  }

  private rowTitle(item: StarMapLibraryEntryView): string {
    const folder = this.folderName(item.entry.parentFolderId)
    return folder.length > 0 ? `${folder} / ${item.title}` : item.title
  }

  private requestCreateFolder(parentFolderId: string = ''): void {
    const args: CompactTextDialogArgs = {
      title: parentFolderId.length > 0 ? '新建子文件夹' : '新建素材文件夹',
      placeholder: '文件夹名称',
      initialText: '',
      confirmLabel: '创建',
      onConfirm: (text: string): void => {
        const name = text.trim()
        if (name.length === 0) {
          return
        }
        this.createFolder(name, parentFolderId)
      }
    }
    openCompactTextDialog(this.getUIContext(), args)
  }

  private async createFolder(name: string, parentFolderId: string): Promise<void> {
    const result = await this.bridge.createStarMapLibraryFolder(
      this.scopeJson, parentFolderId, name)
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '新建文件夹失败'
      return
    }
    await this.reload()
  }

  private async showReferences(target: StarMapAssetRef, title: string): Promise<void> {
    const result = await this.bridge.listStarMapAssetReferences(JSON.stringify(target))
    if (!result.success || result.data === undefined) {
      this.errorMessage = result.userMessage ?? '读取引用位置失败'
      return
    }
    this.references = result.data
    this.referencesTitle = title
    this.referencesSheetVisible = true
  }

  private async copyEntry(item: StarMapLibraryEntryView): Promise<void> {
    const result = await this.bridge.copyStarMapLibraryAsset(
      JSON.stringify(item.entry.target), this.scopeJson, '')
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '复制素材失败'
      return
    }
    await this.reload()
  }

  private async moveEntryToFolder(item: StarMapLibraryEntryView): Promise<void> {
    const result = await this.bridge.moveStarMapLibraryEntry(
      item.entry.entryId, this.scopeJson, this.selectedFolderId)
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '移动素材失败'
      return
    }
    await this.reload()
  }

  private async moveEntryToOtherScope(item: StarMapLibraryEntryView): Promise<void> {
    if (this.otherScopeJson.length === 0) {
      this.errorMessage = '当前没有可移动的目标素材库'
      return
    }
    const result = await this.bridge.moveStarMapLibraryEntry(
      item.entry.entryId, this.otherScopeJson, '')
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '移动素材归属失败'
      return
    }
    await this.reload()
  }

  private async deleteEntry(item: StarMapLibraryEntryView): Promise<void> {
    const refs = await this.bridge.listStarMapAssetReferences(JSON.stringify(item.entry.target))
    if (refs.success && refs.data !== undefined && refs.data.length > 0) {
      await this.showReferences(item.entry.target, item.title)
      return
    }
    const result = await this.bridge.deleteStarMapLibraryAsset(JSON.stringify(item.entry.target))
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '删除素材本体失败'
      return
    }
    await this.reload()
  }

  @Builder
  EntryRow(item: StarMapLibraryEntryView) {
    Row({ space: 8 }) {
      Text(item.entry.target.kind === 'starmap' ? '图' : '点')
        .fontSize(11)
        .fontColor($r('sys.color.font_secondary'))
        .width(20)
      Text(this.rowTitle(item))
        .fontSize(13)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)
      Text('⋯')
        .fontSize(16)
        .fontColor($r('sys.color.font_secondary'))
        .width(28)
        .textAlign(TextAlign.Center)
        .onClick(() => {
          this.menuEntry = item
        })
        .bindMenu(this.EntryMenu)
    }
    .width('100%')
    .height(40)
    .padding({ left: 8, right: 8 })
    .borderRadius(DesignTokens.radiusSm)
    .onClick(() => {
      this.onOpen(item.entry.target, item.title)
    })
    .gesture(LongPressGesture().onAction(() => {
      this.showReferences(item.entry.target, item.title)
    }))
  }

  @Builder
  FolderRow(folder: StarMapLibraryFolder) {
    Row({ space: 6 }) {
      Text(this.selectedFolderId === folder.folderId ? '▾' : '▸')
        .fontSize(11)
        .fontColor($r('sys.color.icon_secondary'))
      Text(folder.name)
        .fontSize(12)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.selectedFolderId === folder.folderId
          ? $r('sys.color.font_primary') : $r('sys.color.font_secondary'))
        .maxLines(1)
        .layoutWeight(1)
      Text('+')
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
        .width(24)
        .textAlign(TextAlign.Center)
        .onClick(() => this.requestCreateFolder(folder.folderId))
    }
    .width('100%')
    .height(32)
    .padding({ left: 4, right: 4 })
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor(this.selectedFolderId === folder.folderId
      ? $r('sys.color.comp_background_gray') : Color.Transparent)
    .onClick(() => {
      this.selectedFolderId = this.selectedFolderId === folder.folderId ? '' : folder.folderId
    })
  }

  @Builder
  EntryMenu() {
    Menu() {
      MenuItem({ content: '插入当前画布' })
        .onClick(() => {
          if (this.canInsert && this.menuEntry !== null) {
            this.onInsert(this.menuEntry.entry.target, this.menuEntry.title)
          }
        })
      MenuItem({ content: '复制' })
        .onClick(() => {
          if (this.menuEntry !== null) {
            this.copyEntry(this.menuEntry)
          }
        })
      MenuItem({ content: this.selectedFolderId.length > 0 ? '移动到当前文件夹' : '移回库根目录' })
        .onClick(() => {
          if (this.menuEntry !== null) {
            this.moveEntryToFolder(this.menuEntry)
          }
        })
      MenuItem({ content: '移动到另一素材库' })
        .onClick(() => {
          if (this.otherScopeJson.length > 0 && this.menuEntry !== null) {
            this.moveEntryToOtherScope(this.menuEntry)
          }
        })
      MenuItem({ content: '查看引用位置' })
        .onClick(() => {
          if (this.menuEntry !== null) {
            this.showReferences(this.menuEntry.entry.target, this.menuEntry.title)
          }
        })
      MenuItem({ content: '删除素材本体' })
        .onClick(() => {
          if (this.menuEntry !== null) {
            this.deleteEntry(this.menuEntry)
          }
        })
    }
  }

  @Builder
  ReferenceSheet() {
    Column({ space: 8 }) {
      Text(`${this.referencesTitle} · 引用位置`)
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .width('100%')
      if (this.references.length === 0) {
        Text('暂无引用')
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
      } else {
        List({ space: 6 }) {
          ForEach(this.references, (ref: StarMapAssetReference) => {
            ListItem() {
              Column({ space: 2 }) {
                Text(ref.starmapId)
                  .fontSize(13)
                  .fontColor($r('sys.color.font_primary'))
                Text(`${ref.projectId ?? '全局'} / ${ref.instanceId}`)
                  .fontSize(11)
                  .fontColor($r('sys.color.font_secondary'))
              }
              .width('100%')
              .padding(8)
              .borderRadius(DesignTokens.radiusSm)
              .backgroundColor($r('sys.color.comp_background_gray'))
              .onClick(() => {
                this.referencesSheetVisible = false
                this.onReferenceJump(ref)
              })
            }
          }, (ref: StarMapAssetReference) => `${ref.starmapId}:${ref.instanceId}`)
        }
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .padding(12)
    .alignItems(HorizontalAlign.Start)
  }

  build() {
    Column() {
      Row() {
        Text(this.scopeLabel)
          .fontSize(13)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .layoutWeight(1)
        Button('+')
          .fontSize(16)
          .width(28)
          .height(28)
          .backgroundColor($r('sys.color.comp_background_gray'))
          .onClick(() => this.requestCreateFolder())
      }
      .width('100%')
      .height(36)

      if (this.loading) {
        Text('加载中…').fontSize(12).fontColor($r('sys.color.font_secondary')).height(32)
      } else if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(11)
          .fontColor($r('sys.color.warning'))
          .maxLines(4)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      } else if (this.scopeJson.length === 0) {
        Text('选择作品后显示作品素材库')
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
      } else if (this.entries.length === 0 && this.folders.length === 0) {
        Text('暂无素材')
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .height(32)
      } else {
        List({ space: 2 }) {
          ForEach(this.folders, (folder: StarMapLibraryFolder) => {
            ListItem() {
              this.FolderRow(folder)
            }
          }, (folder: StarMapLibraryFolder) => folder.folderId)
          ForEach(this.entries, (item: StarMapLibraryEntryView) => {
            ListItem() {
              this.EntryRow(item)
            }
          }, (item: StarMapLibraryEntryView) => item.entry.entryId)
        }
        .width('100%')
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .padding(8)
    .alignItems(HorizontalAlign.Start)
    .bindSheet($$this.referencesSheetVisible, this.ReferenceSheet(), { height: 320 })
  }
}
