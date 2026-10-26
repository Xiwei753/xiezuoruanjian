import { NativeAnnotationBridge } from '../../../corebridge/native/NativeAnnotationBridge'
import {
  AnnotationNamedEntryDto, AnnotationObjectLinkDto, AnnotationObjectRefDto,
  AnnotationTagBindingDto, AnnotationTagDto, AnnotationTargetDto
} from '../../../corebridge/dto/AnnotationDtos'
import { SearchResult } from '../../../corebridge/dto/SearchDtos'
import { getBridge } from '../../../app/AppContext'

interface AnnotationLinkResolutionItem {
  linkId: string
  state: string
}

@Component
export struct AnnotationPanel {
  @Prop projectId: string = ''
  @Prop @Watch('onTargetChanged') target: AnnotationObjectRefDto = { kind: 'chapter', projectId: '', objectId: '' }
  @Prop onChanged: () => void = () => {}

  @State private tags: AnnotationTagDto[] = []
  @State private bindings: AnnotationTagBindingDto[] = []
  @State private namedEntries: AnnotationNamedEntryDto[] = []
  @State private links: AnnotationObjectLinkDto[] = []
  @State private backlinks: AnnotationObjectLinkDto[] = []
  @State private newTagDraft: string = ''
  @State private activeTagId: string = ''
  @State private tagRenameDraft: string = ''
  @State private nameDraft: string = ''
  @State private aliasesDraft: string = ''
  @State private misspellingsDraft: string = ''
  @State private editingEntryId: string = ''
  @State private editCanonicalName: string = ''
  @State private editAliases: string = ''
  @State private editMisspellings: string = ''
  @State private urlDraft: string = ''
  @State private searchDraft: string = ''
  @State private searchResults: SearchResult[] = []
  @State private linkResolutions: AnnotationLinkResolutionItem[] = []
  @State private errorMessage: string = ''
  @State private busy: boolean = false
  private bridge: NativeAnnotationBridge = new NativeAnnotationBridge()

  aboutToAppear(): void {
    void this.refresh()
  }

  private onTargetChanged(): void {
    void this.refresh()
  }

  private async refresh(): Promise<void> {
    if (this.projectId.length === 0 || this.target.projectId.length === 0) return
    const [tagsResult, entriesResult, bindingsResult, linksResult, backlinksResult] = await Promise.all([
      this.bridge.listTags(this.projectId),
      this.bridge.listNamedEntries(this.projectId),
      this.bridge.listBindingsForTarget(this.projectId, this.target),
      this.bridge.listLinksForSource(this.target),
      this.bridge.findBacklinks(this.objectTarget(this.target))
    ])
    if (tagsResult.success && tagsResult.data !== undefined) this.tags = tagsResult.data
    if (entriesResult.success && entriesResult.data !== undefined) this.namedEntries = entriesResult.data
    if (bindingsResult.success && bindingsResult.data !== undefined) this.bindings = bindingsResult.data
    if (linksResult.success && linksResult.data !== undefined) this.links = linksResult.data
    if (backlinksResult.success && backlinksResult.data !== undefined) this.backlinks = backlinksResult.data
    const failure = [tagsResult, entriesResult, bindingsResult, linksResult, backlinksResult]
      .find((result) => !result.success)
    this.errorMessage = failure?.userMessage ?? ''
    const resolutions: AnnotationLinkResolutionItem[] = []
    for (const link of this.links) {
      const result = await this.bridge.resolveLink(link.target)
      if (result.success && result.data !== undefined) {
        resolutions.push({ linkId: link.linkId, state: result.data.state })
      }
    }
    this.linkResolutions = resolutions
  }

  private objectTarget(ref: AnnotationObjectRefDto): AnnotationTargetDto {
    return { objectRef: ref, externalUrl: null }
  }

  private sameObject(left: AnnotationObjectRefDto, right: AnnotationObjectRefDto): boolean {
    return left.kind === right.kind && left.projectId === right.projectId && left.objectId === right.objectId
  }

  private currentTagIds(): string[] {
    return this.bindings
      .filter((binding) => this.sameObject(binding.target, this.target))
      .map((binding) => binding.tagId)
  }

  private async toggleTag(tag: AnnotationTagDto): Promise<void> {
    const ids = this.currentTagIds()
    const index = ids.indexOf(tag.tagId)
    if (index >= 0) ids.splice(index, 1)
    else ids.push(tag.tagId)
    this.busy = true
    const result = await this.bridge.setTagBindings(this.projectId, this.target, ids)
    this.busy = false
    if (!result.success || result.data === undefined) {
      this.errorMessage = result.userMessage ?? '标签未能保存'
      return
    }
    this.bindings = this.bindings.filter((binding) => !this.sameObject(binding.target, this.target))
      .concat(result.data)
    this.errorMessage = ''
    this.onChanged()
  }

  private async createTag(): Promise<void> {
    const name = this.newTagDraft.trim()
    if (name.length === 0) return
    this.busy = true
    const result = await this.bridge.createTag(this.projectId, name)
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '标签创建失败'
      return
    }
    this.newTagDraft = ''
    await this.refresh()
  }

  private async saveTagName(): Promise<void> {
    const name = this.tagRenameDraft.trim()
    if (this.activeTagId.length === 0 || name.length === 0) return
    this.busy = true
    const result = await this.bridge.renameTag(this.projectId, this.activeTagId, name)
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '标签改名失败'
      return
    }
    this.activeTagId = ''
    this.tagRenameDraft = ''
    await this.refresh()
    this.onChanged()
  }

  private async deleteTag(tag: AnnotationTagDto): Promise<void> {
    this.busy = true
    const result = await this.bridge.deleteTag(this.projectId, tag.tagId)
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '标签删除失败'
      return
    }
    if (this.activeTagId === tag.tagId) this.activeTagId = ''
    await this.refresh()
    this.onChanged()
  }

  private async addNamedEntry(): Promise<void> {
    const name = this.nameDraft.trim()
    if (name.length === 0) return
    this.busy = true
    const result = await this.bridge.createNamedEntry(
      this.projectId, name, true, this.splitNames(this.aliasesDraft), this.splitNames(this.misspellingsDraft))
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '专名登记失败'
      return
    }
    this.nameDraft = ''
    this.aliasesDraft = ''
    this.misspellingsDraft = ''
    this.errorMessage = ''
    await this.refresh()
    this.onChanged()
  }

  private splitNames(value: string): string[] {
    const output: string[] = []
    const items = value.split(',')
    for (let i = 0; i < items.length; i++) {
      const item = items[i].trim()
      if (item.length > 0 && !output.includes(item)) output.push(item)
    }
    return output
  }

  private beginEditEntry(entry: AnnotationNamedEntryDto): void {
    this.editingEntryId = entry.entryId
    this.editCanonicalName = entry.canonicalName
    this.editAliases = entry.aliases.join(', ')
    this.editMisspellings = entry.confirmedMisspellings.join(', ')
  }

  private async saveEntry(): Promise<void> {
    const entry = this.namedEntries.find((item: AnnotationNamedEntryDto) => item.entryId === this.editingEntryId)
    if (entry === undefined || this.editCanonicalName.trim().length === 0) return
    const updated: AnnotationNamedEntryDto = {
      entryId: entry.entryId,
      projectId: entry.projectId,
      canonicalName: this.editCanonicalName.trim(),
      aliases: this.splitNames(this.editAliases),
      confirmedMisspellings: this.splitNames(this.editMisspellings),
      autoLinkEnabled: entry.autoLinkEnabled,
      updatedAtMs: entry.updatedAtMs,
      deletedAtMs: entry.deletedAtMs
    }
    const result = await this.bridge.updateNamedEntry(updated)
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '专名更新失败'
      return
    }
    this.editingEntryId = ''
    await this.refresh()
    this.onChanged()
  }

  private async deleteEntry(entry: AnnotationNamedEntryDto): Promise<void> {
    const result = await this.bridge.deleteNamedEntry(this.projectId, entry.entryId)
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '专名删除失败'
      return
    }
    if (this.editingEntryId === entry.entryId) this.editingEntryId = ''
    await this.refresh()
    this.onChanged()
  }

  private async toggleAutoLink(entry: AnnotationNamedEntryDto): Promise<void> {
    const updated: AnnotationNamedEntryDto = {
      entryId: entry.entryId,
      projectId: entry.projectId,
      canonicalName: entry.canonicalName,
      aliases: entry.aliases,
      confirmedMisspellings: entry.confirmedMisspellings,
      autoLinkEnabled: !entry.autoLinkEnabled,
      updatedAtMs: entry.updatedAtMs,
      deletedAtMs: entry.deletedAtMs
    }
    const result = await this.bridge.updateNamedEntry(updated)
    if (!result.success || result.data === undefined) {
      this.errorMessage = result.userMessage ?? '专名设置未能保存'
      return
    }
    this.namedEntries = this.namedEntries.map((item) => item.entryId === updated.entryId ? result.data as AnnotationNamedEntryDto : item)
    this.errorMessage = ''
    this.onChanged()
  }

  private async addExternalLink(): Promise<void> {
    const url = this.urlDraft.trim()
    if (!url.startsWith('https://') && !url.startsWith('http://')) {
      this.errorMessage = '请输入 http 或 https 链接'
      return
    }
    this.busy = true
    const result = await this.bridge.createLink(this.target, { objectRef: null, externalUrl: url })
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '链接未能保存'
      return
    }
    this.urlDraft = ''
    this.errorMessage = ''
    await this.refresh()
    this.onChanged()
  }

  private async searchLinkTargets(): Promise<void> {
    const query = this.searchDraft.trim()
    if (query.length === 0) {
      this.searchResults = []
      return
    }
    const result = await getBridge().globalSearchPage(query, 'all', 20, null, {
      originScreen: 'writing', currentProjectId: this.projectId
    })
    if (!result.success || result.data === undefined) {
      this.errorMessage = result.userMessage ?? '对象搜索失败'
      this.searchResults = []
      return
    }
    this.searchResults = result.data.items
    this.errorMessage = ''
  }

  private objectRefForResult(result: SearchResult): AnnotationObjectRefDto | null {
    const target = result.target
    const projectId = target.projectId ?? this.projectId
    if (projectId.length === 0) return null
    const entryPrefix = `annotation:${projectId}:entry:`
    if (result.objectId.startsWith(entryPrefix)) {
      return { kind: 'namedEntry', projectId: projectId,
        objectId: result.objectId.substring(entryPrefix.length) }
    }
    if (target.nodeId !== null) return { kind: 'starMapNode', projectId: projectId, objectId: target.nodeId }
    if (target.embedId !== null) return { kind: 'starMapEmbed', projectId: projectId, objectId: target.embedId }
    if (target.starmapId !== null) return { kind: 'starMap', projectId: projectId, objectId: target.starmapId }
    if (target.chapterId !== null) return { kind: 'chapter', projectId: projectId, objectId: target.chapterId }
    if (target.volumeId !== null) return { kind: 'volume', projectId: projectId, objectId: target.volumeId }
    return { kind: 'project', projectId: projectId, objectId: projectId }
  }

  private async addObjectLink(result: SearchResult): Promise<void> {
    const ref = this.objectRefForResult(result)
    if (ref === null) return
    this.busy = true
    const created = await this.bridge.createLink(this.target, { objectRef: ref, externalUrl: null })
    this.busy = false
    if (!created.success) {
      this.errorMessage = created.userMessage ?? '对象链接未能保存'
      return
    }
    this.searchDraft = ''
    this.searchResults = []
    this.errorMessage = ''
    await this.refresh()
    this.onChanged()
  }

  private async deleteLink(link: AnnotationObjectLinkDto): Promise<void> {
    this.busy = true
    const result = await this.bridge.deleteLink(link.source.projectId, link.linkId)
    this.busy = false
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '链接删除失败'
      return
    }
    await this.refresh()
    this.onChanged()
  }

  private linkStateLabel(linkId: string): string {
    const item = this.linkResolutions.find((value: AnnotationLinkResolutionItem) => value.linkId === linkId)
    if (item === undefined) return '状态未知'
    if (item.state === 'exists') return '可访问'
    if (item.state === 'missing') return '目标缺失'
    if (item.state === 'projectNotDownloaded') return '项目未下载'
    return '外部网址'
  }

  build() {
    Column({ space: 8 }) {
      Row() {
        Text('标签与专名')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
        Blank()
        Text(`${this.target.kind}`)
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
      }
      .width('100%')

      Scroll() {
        Column({ space: 10 }) {
          Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
            ForEach(this.tags, (tag: AnnotationTagDto) => {
              Row({ space: 4 }) {
                Button(`${this.currentTagIds().includes(tag.tagId) ? '✓ ' : ''}${tag.name}`)
                  .height(34)
                  .enabled(!this.busy)
                  .onClick(() => { void this.toggleTag(tag) })
                Button('改名')
                  .height(30)
                  .enabled(!this.busy)
                  .onClick(() => {
                    this.activeTagId = tag.tagId
                    this.tagRenameDraft = tag.name
                  })
                Button('删除')
                  .height(30)
                  .enabled(!this.busy)
                  .onClick(() => { void this.deleteTag(tag) })
              }
              .margin({ right: 6, bottom: 6 })
            }, (tag: AnnotationTagDto) => tag.tagId)
          }

          Row({ space: 8 }) {
            TextInput({ text: this.newTagDraft, placeholder: '新建标签' })
              .layoutWeight(1)
              .onChange((value: string) => { this.newTagDraft = value })
            Button('创建标签')
              .enabled(!this.busy && this.newTagDraft.trim().length > 0)
              .onClick(() => { void this.createTag() })
          }
          if (this.activeTagId.length > 0) {
            Row({ space: 8 }) {
              TextInput({ text: this.tagRenameDraft, placeholder: '标签名称' })
                .layoutWeight(1)
                .onChange((value: string) => { this.tagRenameDraft = value })
              Button('保存改名')
                .enabled(!this.busy && this.tagRenameDraft.trim().length > 0)
                .onClick(() => { void this.saveTagName() })
            }
          }

          Row({ space: 8 }) {
            TextInput({ text: this.nameDraft, placeholder: '登记人物、地点等专名' })
              .layoutWeight(1)
              .onChange((value: string) => { this.nameDraft = value })
            Button('登记并自动链接')
              .enabled(!this.busy && this.nameDraft.trim().length > 0)
              .onClick(() => { void this.addNamedEntry() })
          }
          TextInput({ text: this.aliasesDraft, placeholder: '别名（逗号分隔，可选）' })
            .width('100%')
            .onChange((value: string) => { this.aliasesDraft = value })
          TextInput({ text: this.misspellingsDraft, placeholder: '确认错写（逗号分隔，可选）' })
            .width('100%')
            .onChange((value: string) => { this.misspellingsDraft = value })

          ForEach(this.namedEntries, (entry: AnnotationNamedEntryDto) => {
            Row() {
              Text(entry.canonicalName)
                .fontSize(14)
              Blank()
              Button(entry.autoLinkEnabled ? '自动链接：开' : '自动链接：关')
                .onClick(() => { void this.toggleAutoLink(entry) })
              Button('编辑').onClick(() => { this.beginEditEntry(entry) })
              Button('删除').onClick(() => { void this.deleteEntry(entry) })
            }
            .width('100%')
          }, (entry: AnnotationNamedEntryDto) => entry.entryId)
          if (this.editingEntryId.length > 0) {
            TextInput({ text: this.editCanonicalName, placeholder: '规范名称' })
              .width('100%')
              .onChange((value: string) => { this.editCanonicalName = value })
            TextInput({ text: this.editAliases, placeholder: '别名（逗号分隔）' })
              .width('100%')
              .onChange((value: string) => { this.editAliases = value })
            TextInput({ text: this.editMisspellings, placeholder: '确认错写（逗号分隔）' })
              .width('100%')
              .onChange((value: string) => { this.editMisspellings = value })
            Button('保存专名').onClick(() => { void this.saveEntry() })
          }

          Row({ space: 8 }) {
            TextInput({ text: this.searchDraft, placeholder: '搜索作品、章节或星图对象' })
              .layoutWeight(1)
              .onChange((value: string) => { this.searchDraft = value })
              .onSubmit(() => { void this.searchLinkTargets() })
            Button('搜索')
              .enabled(!this.busy && this.searchDraft.trim().length > 0)
              .onClick(() => { void this.searchLinkTargets() })
          }
          ForEach(this.searchResults, (result: SearchResult) => {
            Button() {
              Text(`${result.title} · ${result.path}`)
                .maxLines(2)
            }
              .width('100%')
              .enabled(!this.busy && this.objectRefForResult(result) !== null)
              .onClick(() => { void this.addObjectLink(result) })
          }, (result: SearchResult) => result.objectId)

          Row({ space: 8 }) {
            TextInput({ text: this.urlDraft, placeholder: 'http(s) 链接' })
              .layoutWeight(1)
              .onChange((value: string) => { this.urlDraft = value })
            Button('添加链接')
              .enabled(!this.busy && this.urlDraft.trim().length > 0)
              .onClick(() => { void this.addExternalLink() })
          }

          if (this.links.length > 0) {
            Text('关联链接').fontSize(13).fontColor($r('sys.color.font_secondary'))
            ForEach(this.links, (link: AnnotationObjectLinkDto) => {
              Row() {
                Column() {
                  Text(link.target.externalUrl ?? `${link.target.objectRef?.kind ?? '对象'} · ${link.target.objectRef?.objectId ?? ''}`)
                    .fontSize(13)
                    .maxLines(2)
                  Text(this.linkStateLabel(link.linkId))
                    .fontSize(11)
                    .fontColor($r('sys.color.font_secondary'))
                }
                .layoutWeight(1)
                Button('删除')
                  .enabled(!this.busy)
                  .onClick(() => { void this.deleteLink(link) })
              }
              .width('100%')
            }, (link: AnnotationObjectLinkDto) => link.linkId)
          }
          if (this.backlinks.length > 0) {
            Text(`反向链接 ${this.backlinks.length}`)
              .fontSize(13)
              .fontColor($r('sys.color.font_secondary'))
          }
          if (this.errorMessage.length > 0) {
            Text(this.errorMessage)
              .fontSize(12)
              .fontColor($r('sys.color.ohos_id_color_alert'))
          }
        }
        .width('100%')
      }
      .height(250)
    }
    .width('100%')
    .padding(12)
    .backgroundColor($r('sys.color.background_primary'))
    .borderRadius(12)
  }
}
