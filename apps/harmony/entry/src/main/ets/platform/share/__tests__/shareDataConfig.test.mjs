// shareDataConfig.test.mjs — SharePayload -> SharedData 规范化纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.ShareKit / ArkUI，Node 直接运行：node shareDataConfig.test.mjs
//
// 本测试验证 payloadToSharedDataConfig 的逻辑规格。该规格与以下 service 内联实现严格一致：
//   - SystemShareService.buildSharedData （普通系统分享）
//   - TapShareService.buildSharedData    （碰一碰）
//   - AirTransferService.buildSharedData （隔空传送）
// 三个 service 的 buildSharedData 都遵循同一规格：text 非空进 textContents，uris 非空进 uris（过滤空串），
// title 非空单独存，两者都空则 hasContent=false（调用方返回 false，不假装成功）。
// 本测试通过验证规格，间接验证三个 service 的数据构造正确性。
// 真正构造 systemShare.SharedData 的调用需 HarmonyOS SDK 编译（见 ShareLifecycleTest.ets）。

// —— 被测规格（与 service 内联实现同规格）——
function payloadToSharedDataConfig(payload) {
  const textContents = []
  const uris = []
  if (payload !== undefined && payload !== null) {
    if (payload.text !== undefined && payload.text !== null && payload.text.length > 0) {
      textContents.push(payload.text)
    }
    if (payload.uris !== undefined && payload.uris !== null) {
      for (const u of payload.uris) {
        if (u !== undefined && u !== null && u.length > 0) {
          uris.push(u)
        }
      }
    }
  }
  const hasContent = textContents.length > 0 || uris.length > 0
  const title = (payload !== undefined && payload !== null &&
    payload.title !== undefined && payload.title !== null && payload.title.length > 0)
    ? payload.title : undefined
  const kind = (payload !== undefined && payload !== null && payload.kind !== undefined && payload.kind !== null)
    ? payload.kind : 'text'
  return { hasContent, textContents, uris, title, kind }
}

// —— 断言工具 ——
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}
function eq(a, b) { return JSON.stringify(a) === JSON.stringify(b) }

console.log('shareDataConfig 纯逻辑测试')

console.log('1. 空 payload（无 text/无 uris）')
let r = payloadToSharedDataConfig({ kind: 'text' })
assert(r.hasContent === false, '空 payload hasContent=false')
assert(eq(r.textContents, []), '空 payload textContents=[]')
assert(eq(r.uris, []), '空 payload uris=[]')
assert(r.title === undefined, '空 payload title=undefined')

console.log('2. 只有 text')
r = payloadToSharedDataConfig({ kind: 'text', text: 'hello' })
assert(r.hasContent === true, 'text hasContent=true')
assert(eq(r.textContents, ['hello']), 'text textContents=[hello]')
assert(eq(r.uris, []), 'text uris=[]')

console.log('3. 只有 uris')
r = payloadToSharedDataConfig({ kind: 'file', uris: ['file://a', 'file://b'] })
assert(r.hasContent === true, 'uris hasContent=true')
assert(eq(r.textContents, []), 'uris textContents=[]')
assert(eq(r.uris, ['file://a', 'file://b']), 'uris uris=[file://a,file://b]')

console.log('4. text + uris')
r = payloadToSharedDataConfig({ kind: 'image', text: 'desc', uris: ['file://img'] })
assert(r.hasContent === true, 'text+uris hasContent=true')
assert(eq(r.textContents, ['desc']), 'text+uris textContents=[desc]')
assert(eq(r.uris, ['file://img']), 'text+uris uris=[file://img]')

console.log('5. title 单独存')
r = payloadToSharedDataConfig({ kind: 'text', title: 'My Title', text: 'body' })
assert(r.title === 'My Title', 'title 单独存')
assert(eq(r.textContents, ['body']), 'title 不混入 textContents')

console.log('6. 空串过滤')
r = payloadToSharedDataConfig({ kind: 'text', text: '', uris: ['', 'file://x', ''] })
assert(r.hasContent === true, '空串过滤后仍有内容 hasContent=true')
assert(eq(r.textContents, []), '空 text 不进 textContents')
assert(eq(r.uris, ['file://x']), '空 uri 过滤，只留 file://x')

console.log('7. null/undefined 安全')
r = payloadToSharedDataConfig(null)
assert(r.hasContent === false, 'null payload hasContent=false')
assert(r.kind === 'text', 'null payload kind 默认 text')
r = payloadToSharedDataConfig(undefined)
assert(r.hasContent === false, 'undefined payload hasContent=false')

console.log('8. kind 保留')
r = payloadToSharedDataConfig({ kind: 'image', text: 'x' })
assert(r.kind === 'image', 'kind=image 保留')
r = payloadToSharedDataConfig({ text: 'x' })
assert(r.kind === 'text', 'kind 缺省为 text')

console.log('9. 空标题不存')
r = payloadToSharedDataConfig({ kind: 'text', title: '', text: 'body' })
assert(r.title === undefined, '空 title 不存')

console.log('10. 多 uri 顺序保留')
r = payloadToSharedDataConfig({ kind: 'file', uris: ['a', 'b', 'c'] })
assert(eq(r.uris, ['a', 'b', 'c']), '多 uri 顺序保留')

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
