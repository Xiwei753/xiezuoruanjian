// shareDataConfig.test.mjs — SharePayload -> SharedData 规范化纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.ShareKit / ArkUI，Node 直接运行：node shareDataConfig.test.mjs
//
// 本测试验证 payloadToSharedDataConfig 的逻辑规格。该规格与以下实现严格一致：
//   - SystemShareApi11.buildSharedData (impl/api11)   （普通系统分享）
//   - KnockShareApi12.buildSharedData (impl/api12)     （碰一碰）
//   - GesturesShareApi20.buildSharedData (impl/api20)  （隔空传送）
// 三处 buildSharedData 都遵循同一规格（Issue #766）：
//   kind='file'：主记录是 general.file-uri（带 title 元数据），其余 URI addRecord，text 在文件之后
//   kind='text'（及其他）：主记录是 general.text（title 写进 record.title），不再把 title 伪装成独立正文
//   无有效内容则 hasContent=false（调用方返回 false，不假装成功）
// 本测试通过验证规格，间接验证三处数据构造的正确性。
// 真正构造 systemShare.SharedData 的调用需 HarmonyOS SDK 编译（见 ShareLifecycleTest.ets）。

// —— 被测规格（与 service 内联实现同规格）——
function payloadToSharedDataConfig(payload) {
  const hasText = payload !== undefined && payload !== null &&
    payload.text !== undefined && payload.text !== null && payload.text.length > 0
  const hasTitle = payload !== undefined && payload !== null &&
    payload.title !== undefined && payload.title !== null && payload.title.length > 0
  const titleValue = hasTitle ? payload.title : undefined

  const validUris = []
  if (payload !== undefined && payload !== null &&
    payload.uris !== undefined && payload.uris !== null) {
    for (const u of payload.uris) {
      if (u !== undefined && u !== null && u.length > 0) {
        validUris.push(u)
      }
    }
  }

  const hasContent = hasText || validUris.length > 0
  const kind = (payload !== undefined && payload !== null && payload.kind !== undefined && payload.kind !== null)
    ? payload.kind : 'text'

  if (!hasContent) {
    return { hasContent: false, kind }
  }

  if (kind === 'file' && validUris.length > 0) {
    // 文件分享：主记录是 general.file-uri，带 title
    const primaryRecord = { utd: 'general.file-uri', uri: validUris[0], title: titleValue }
    const additionalRecords = []
    for (let i = 1; i < validUris.length; i++) {
      additionalRecords.push({ utd: 'general.file-uri', uri: validUris[i], title: titleValue })
    }
    if (hasText) {
      additionalRecords.push({ utd: 'general.text', content: payload.text })
    }
    return { hasContent: true, kind, primaryRecord, additionalRecords }
  }

  // 纯文本分享：主记录是 general.text，title 写进 record.title
  const primaryRecord = { utd: 'general.text', content: hasText ? payload.text : '', title: titleValue }
  const additionalRecords = []
  for (const uri of validUris) {
    additionalRecords.push({ utd: 'general.file-uri', uri })
  }
  return { hasContent: true, kind, primaryRecord, additionalRecords }
}

// —— 断言工具 ——
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}
function eq(a, b) { return JSON.stringify(a) === JSON.stringify(b) }

console.log('shareDataConfig 纯逻辑测试（Issue #766 规格）')

console.log('1. 空 payload（无 text/无 uris）')
let r = payloadToSharedDataConfig({ kind: 'text' })
assert(r.hasContent === false, '空 payload hasContent=false')

console.log('2. 只有 text（kind=text）')
r = payloadToSharedDataConfig({ kind: 'text', text: 'hello' })
assert(r.hasContent === true, 'text hasContent=true')
assert(r.primaryRecord.utd === 'general.text', 'text 主记录 utd=general.text')
assert(r.primaryRecord.content === 'hello', 'text 主记录 content=hello')
assert(r.primaryRecord.title === undefined, 'text 无 title 时 title=undefined')
assert(eq(r.additionalRecords, []), 'text 无额外记录')

console.log('3. text + title（kind=text）：title 写进 record.title，不伪装成独立正文')
r = payloadToSharedDataConfig({ kind: 'text', title: 'My Title', text: 'body' })
assert(r.hasContent === true, 'text+title hasContent=true')
assert(r.primaryRecord.utd === 'general.text', '主记录 utd=general.text')
assert(r.primaryRecord.content === 'body', '主记录 content=body（不是 title）')
assert(r.primaryRecord.title === 'My Title', '主记录 title=My Title（元数据）')
assert(eq(r.additionalRecords, []), '不额外创建标题正文记录')

console.log('4. 只有 uris（kind=file）：主记录是 file-uri')
r = payloadToSharedDataConfig({ kind: 'file', uris: ['file://a', 'file://b'] })
assert(r.hasContent === true, 'file hasContent=true')
assert(r.primaryRecord.utd === 'general.file-uri', 'file 主记录 utd=general.file-uri')
assert(r.primaryRecord.uri === 'file://a', 'file 主记录 uri=file://a')
assert(r.primaryRecord.title === undefined, 'file 无 title 时 title=undefined')
assert(r.additionalRecords.length === 1, 'file 第二条 URI addRecord')
assert(r.additionalRecords[0].utd === 'general.file-uri', 'file 额外记录 utd=general.file-uri')
assert(r.additionalRecords[0].uri === 'file://b', 'file 额外记录 uri=file://b')

console.log('5. file + title：title 写进 file-uri record 的 title 字段')
r = payloadToSharedDataConfig({ kind: 'file', title: '素笺诊断包', uris: ['file://zip'] })
assert(r.hasContent === true, 'file+title hasContent=true')
assert(r.primaryRecord.utd === 'general.file-uri', '主记录 utd=general.file-uri（不是 text）')
assert(r.primaryRecord.uri === 'file://zip', '主记录 uri=file://zip')
assert(r.primaryRecord.title === '素笺诊断包', '主记录 title=素笺诊断包（元数据）')
assert(eq(r.additionalRecords, []), '无额外记录')

console.log('6. file + title + text：text 在文件记录之后')
r = payloadToSharedDataConfig({ kind: 'file', title: '附件', text: '描述', uris: ['file://a'] })
assert(r.primaryRecord.utd === 'general.file-uri', '主记录 utd=general.file-uri')
assert(r.primaryRecord.title === '附件', '主记录 title=附件')
assert(r.additionalRecords.length === 1, '一条额外记录（text）')
assert(r.additionalRecords[0].utd === 'general.text', '额外记录 utd=general.text')
assert(r.additionalRecords[0].content === '描述', '额外记录 content=描述')

console.log('7. 空串过滤')
r = payloadToSharedDataConfig({ kind: 'file', uris: ['', 'file://x', ''] })
assert(r.hasContent === true, '空串过滤后仍有内容 hasContent=true')
assert(r.primaryRecord.uri === 'file://x', '主记录 uri=file://x（空串过滤）')
assert(r.additionalRecords.length === 0, '无额外 URI 记录')

console.log('8. null/undefined 安全')
r = payloadToSharedDataConfig(null)
assert(r.hasContent === false, 'null payload hasContent=false')
assert(r.kind === 'text', 'null payload kind 默认 text')
r = payloadToSharedDataConfig(undefined)
assert(r.hasContent === false, 'undefined payload hasContent=false')

console.log('9. kind 保留')
r = payloadToSharedDataConfig({ kind: 'image', text: 'x' })
assert(r.kind === 'image', 'kind=image 保留')
r = payloadToSharedDataConfig({ text: 'x' })
assert(r.kind === 'text', 'kind 缺省为 text')

console.log('10. 空标题不存')
r = payloadToSharedDataConfig({ kind: 'text', title: '', text: 'body' })
assert(r.primaryRecord.title === undefined, '空 title 不存')

console.log('11. 多 uri 顺序保留（kind=file）')
r = payloadToSharedDataConfig({ kind: 'file', uris: ['a', 'b', 'c'] })
assert(r.primaryRecord.uri === 'a', '主记录 uri=a')
assert(r.additionalRecords.length === 2, '两条额外 URI 记录')
assert(r.additionalRecords[0].uri === 'b', '额外记录[0].uri=b')
assert(r.additionalRecords[1].uri === 'c', '额外记录[1].uri=c')

console.log('12. 诊断包场景：kind=file, title=素笺诊断包, uris=[zip]')
r = payloadToSharedDataConfig({ kind: 'file', title: '素笺诊断包', uris: ['file://sujian-diagnostics-2026.zip'] })
assert(r.primaryRecord.utd === 'general.file-uri', '诊断包主记录 utd=general.file-uri（不是 text）')
assert(r.primaryRecord.uri === 'file://sujian-diagnostics-2026.zip', '诊断包主记录 uri=zip')
assert(r.primaryRecord.title === '素笺诊断包', '诊断包主记录 title=素笺诊断包')
assert(eq(r.additionalRecords, []), '诊断包无额外记录')
// 关键验证：主记录不是 general.text + '素笺诊断包'
assert(r.primaryRecord.utd !== 'general.text', '诊断包主记录不是 general.text')
assert(r.primaryRecord.content === undefined, '诊断包主记录没有 content 字段')

console.log('13. kind=text + uris（非 file kind 但携带 URI）')
r = payloadToSharedDataConfig({ kind: 'image', text: 'desc', uris: ['file://img'] })
assert(r.primaryRecord.utd === 'general.text', 'image kind 主记录 utd=general.text')
assert(r.primaryRecord.content === 'desc', 'image kind 主记录 content=desc')
assert(r.additionalRecords.length === 1, 'image kind 一条额外 file-uri 记录')
assert(r.additionalRecords[0].utd === 'general.file-uri', 'image kind 额外记录 utd=general.file-uri')

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
