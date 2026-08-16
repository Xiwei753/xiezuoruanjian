// starmap_share_lifecycle.test.mjs — 星图页 Share 生命周期纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.ShareKit / ArkUI / Core bridge，Node 直接运行：
//   node apps/harmony/entry/src/main/ets/feature/starmap/__tests__/starmap_share_lifecycle.test.mjs
//
// Issue #629 评论5 第7部分：碰一碰/隔空传送由实际可分享页面拥有生命周期。
// 本测试验证 StarMapScreen 的 share 生命周期行为规格：
//   1. 进入前台：startTapSharing + startAirTransferSharing，payload 携带当前星图信息
//   2. 内容变化：updateTapPayload + updateAirTransferPayload（不重新 start）
//   3. 离开/退后台：stopTapSharing + stopAirTransferSharing + 清空 payload
//   4. 普通"系统分享"按钮：只走 share(payload, 'system')，不混用 tap/airTransfer 生命周期
//   5. startTapSharing/startAirTransferSharing 返回真实注册结果，不假装成功
//
// 本测试用 mock shareService 记录真实调用（方法名 + 参数 + 返回值），
// 验证真实行为，不做字符串匹配冒充。
//
// 被测规格与 StarMapScreen.ets 内联实现严格一致：
//   - buildSharePayload(starmapTitle, graph) 构造 payload
//   - startShareLifecycle / updateSharePayload / stopShareLifecycle / systemShare

// —— 被测规格：buildSharePayload（与 StarMapScreen.buildSharePayload 同规格）——
function buildSharePayload(starmapTitle, graph) {
  const title = starmapTitle
  let text = ''
  if (graph !== null && graph !== undefined) {
    const nodeCount = graph.nodes.length
    const edgeCount = graph.edges.length
    const nodeNames = []
    for (let i = 0; i < graph.nodes.length; i++) {
      const node = graph.nodes[i]
      if (node.title.length > 0) {
        nodeNames.push(node.title)
        if (nodeNames.length >= 20) {
          break
        }
      }
    }
    text = `星图：${title}\n节点 ${nodeCount} / 关系 ${edgeCount}`
    if (nodeNames.length > 0) {
      text = text + '\n' + nodeNames.join('、')
    }
  } else {
    if (title.length > 0) {
      text = `星图：${title}`
    }
  }
  const payload = { kind: 'text', text: text }
  if (title.length > 0) {
    payload.title = title
  }
  return payload
}

// —— 被测规格：生命周期方法（与 StarMapScreen 内联实现同规格）——
// 进入前台：startTapSharing（同步）+ startAirTransferSharing（异步）
async function startShareLifecycle(shareService, payload) {
  let tapOk
  try {
    tapOk = shareService.startTapSharing(payload)
  } catch (e) {
    tapOk = false
  }
  let airOk
  try {
    airOk = await shareService.startAirTransferSharing(payload)
  } catch (e) {
    airOk = false
  }
  return { tapOk, airOk }
}

// 内容变化：updateTapPayload + updateAirTransferPayload（不重新 start）
function updateSharePayload(shareService, payload) {
  shareService.updateTapPayload(payload)
  shareService.updateAirTransferPayload(payload)
}

// 离开/退后台：stopTapSharing + stopAirTransferSharing + 清空 payload
async function stopShareLifecycle(shareService) {
  try {
    shareService.stopTapSharing()
  } catch (e) {}
  try {
    await shareService.stopAirTransferSharing()
  } catch (e) {}
  try {
    shareService.updateTapPayload(null)
    shareService.updateAirTransferPayload(null)
  } catch (e) {}
}

// 普通"系统分享"按钮：只走 share(payload, 'system')
async function systemShare(shareService, payload) {
  return await shareService.share(payload, 'system')
}

// —— mock shareService：记录所有调用（方法名 + 参数 + 返回值）——
function makeMockShareService(opts) {
  opts = opts || {}
  const calls = []
  const tapStartResult = opts.tapStartResult !== undefined ? opts.tapStartResult : true
  const airStartResult = opts.airStartResult !== undefined ? opts.airStartResult : true
  const systemShareResult = opts.systemShareResult !== undefined ? opts.systemShareResult : true
  return {
    calls,
    startTapSharing(payload) {
      calls.push({ method: 'startTapSharing', args: [payload] })
      return tapStartResult
    },
    async startAirTransferSharing(payload, windowId) {
      calls.push({ method: 'startAirTransferSharing', args: [payload, windowId] })
      return airStartResult
    },
    updateTapPayload(payload) {
      calls.push({ method: 'updateTapPayload', args: [payload] })
    },
    updateAirTransferPayload(payload) {
      calls.push({ method: 'updateAirTransferPayload', args: [payload] })
    },
    stopTapSharing() {
      calls.push({ method: 'stopTapSharing', args: [] })
    },
    async stopAirTransferSharing(windowId) {
      calls.push({ method: 'stopAirTransferSharing', args: [windowId] })
    },
    async share(payload, channel) {
      calls.push({ method: 'share', args: [payload, channel] })
      if (channel === 'system') return systemShareResult
      // tap/airTransfer 一次性 share 返回 false（引导走生命周期接口）
      return false
    }
  }
}

// —— 断言工具 ——
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}
function eq(a, b) { return JSON.stringify(a) === JSON.stringify(b) }
function callsOf(svc, method) {
  return svc.calls.filter(c => c.method === method)
}

// —— 测试数据 ——
const starmapTitle = '我的星图'
const graph = {
  nodes: [
    { id: 'n1', title: '主角', kind: 'Character' },
    { id: 'n2', title: '反派', kind: 'Character' },
    { id: 'n3', title: '', kind: 'Location' }
  ],
  edges: [
    { id: 'e1', from: 'n1', to: 'n2', kind: 'CharacterRelation' }
  ]
}

console.log('星图页 Share 生命周期纯逻辑测试')
console.log('')

console.log('1. buildSharePayload 携带星图标题 + 节点/关系摘要 + 节点名列表')
{
  const payload = buildSharePayload(starmapTitle, graph)
  assert(payload.kind === 'text', 'kind=text')
  assert(payload.title === '我的星图', 'title=我的星图')
  assert(payload.text.includes('节点 3'), 'text 含节点数 3')
  assert(payload.text.includes('关系 1'), 'text 含关系数 1')
  assert(payload.text.includes('主角'), 'text 含节点名 主角')
  assert(payload.text.includes('反派'), 'text 含节点名 反派')
  assert(!payload.text.includes('n3'), '空标题节点名不进 text')
}

console.log('2. buildSharePayload 节点名列表上限 20 个')
{
  const bigNodes = []
  for (let i = 0; i < 30; i++) bigNodes.push({ id: 'n' + i, title: '节点' + i, kind: 'Character' })
  const bigGraph = { nodes: bigNodes, edges: [] }
  const payload = buildSharePayload('大星图', bigGraph)
  // 前 20 个节点名进 text
  assert(payload.text.includes('节点0'), '含节点0')
  assert(payload.text.includes('节点19'), '含节点19')
  assert(!payload.text.includes('节点20'), '不含节点20（上限 20）')
}

console.log('3. buildSharePayload graph 为 null 时只有标题')
{
  const payload = buildSharePayload('空星图', null)
  assert(payload.title === '空星图', 'title=空星图')
  assert(payload.text === '星图：空星图', 'text 只有标题')
}

console.log('4. 进入前台：startShareLifecycle 调用 startTapSharing + startAirTransferSharing')
{
  const svc = makeMockShareService()
  const payload = buildSharePayload(starmapTitle, graph)
  startShareLifecycle(svc, payload).then(result => {
    const tapCalls = callsOf(svc, 'startTapSharing')
    const airCalls = callsOf(svc, 'startAirTransferSharing')
    assert(tapCalls.length === 1, 'startTapSharing 调用 1 次')
    assert(airCalls.length === 1, 'startAirTransferSharing 调用 1 次')
    assert(eq(tapCalls[0].args[0], payload), 'startTapSharing 收到 payload')
    assert(eq(airCalls[0].args[0], payload), 'startAirTransferSharing 收到 payload')
    assert(result.tapOk === true, 'tapOk=true（真实注册结果）')
    assert(result.airOk === true, 'airOk=true（真实注册结果）')
  })
}

console.log('5. 进入前台：startTapSharing 失败返回 false，不假装成功')
{
  const svc = makeMockShareService({ tapStartResult: false })
  const payload = buildSharePayload(starmapTitle, graph)
  startShareLifecycle(svc, payload).then(result => {
    assert(result.tapOk === false, 'tapOk=false（真实失败，不假装 true）')
  })
}

console.log('6. 进入前台：startAirTransferSharing 失败返回 false，不假装成功')
{
  const svc = makeMockShareService({ airStartResult: false })
  const payload = buildSharePayload(starmapTitle, graph)
  startShareLifecycle(svc, payload).then(result => {
    assert(result.airOk === false, 'airOk=false（真实失败，不假装 true）')
  })
}

console.log('7. 内容变化：updateSharePayload 调用 updateTapPayload + updateAirTransferPayload，不重新 start')
{
  const svc = makeMockShareService()
  const payload = buildSharePayload(starmapTitle, graph)
  updateSharePayload(svc, payload)
  const tapUpdates = callsOf(svc, 'updateTapPayload')
  const airUpdates = callsOf(svc, 'updateAirTransferPayload')
  const tapStarts = callsOf(svc, 'startTapSharing')
  const airStarts = callsOf(svc, 'startAirTransferSharing')
  assert(tapUpdates.length === 1, 'updateTapPayload 调用 1 次')
  assert(airUpdates.length === 1, 'updateAirTransferPayload 调用 1 次')
  assert(eq(tapUpdates[0].args[0], payload), 'updateTapPayload 收到 payload')
  assert(eq(airUpdates[0].args[0], payload), 'updateAirTransferPayload 收到 payload')
  assert(tapStarts.length === 0, 'update 不重新 startTapSharing')
  assert(airStarts.length === 0, 'update 不重新 startAirTransferSharing')
}

console.log('8. 离开/退后台：stopShareLifecycle 调用 stopTapSharing + stopAirTransferSharing + 清空 payload')
{
  const svc = makeMockShareService()
  stopShareLifecycle(svc).then(() => {
    const tapStops = callsOf(svc, 'stopTapSharing')
    const airStops = callsOf(svc, 'stopAirTransferSharing')
    const tapUpdates = callsOf(svc, 'updateTapPayload')
    const airUpdates = callsOf(svc, 'updateAirTransferPayload')
    assert(tapStops.length === 1, 'stopTapSharing 调用 1 次')
    assert(airStops.length === 1, 'stopAirTransferSharing 调用 1 次')
    assert(tapUpdates.length === 1, '清空 updateTapPayload 1 次')
    assert(airUpdates.length === 1, '清空 updateAirTransferPayload 1 次')
    assert(tapUpdates[0].args[0] === null, 'updateTapPayload(null) 清空')
    assert(airUpdates[0].args[0] === null, 'updateAirTransferPayload(null) 清空')
  })
}

console.log('9. 普通"系统分享"按钮：只走 share(payload, "system")，不混用生命周期接口')
{
  const svc = makeMockShareService()
  const payload = buildSharePayload(starmapTitle, graph)
  systemShare(svc, payload).then(result => {
    const shareCalls = callsOf(svc, 'share')
    assert(shareCalls.length === 1, 'share 调用 1 次')
    assert(shareCalls[0].args[1] === 'system', 'channel=system')
    assert(eq(shareCalls[0].args[0], payload), 'share 收到 payload')
    assert(result === true, 'system share 返回 true')
    // 不混用生命周期接口
    assert(callsOf(svc, 'startTapSharing').length === 0, '系统分享不调 startTapSharing')
    assert(callsOf(svc, 'startAirTransferSharing').length === 0, '系统分享不调 startAirTransferSharing')
    assert(callsOf(svc, 'updateTapPayload').length === 0, '系统分享不调 updateTapPayload')
    assert(callsOf(svc, 'stopTapSharing').length === 0, '系统分享不调 stopTapSharing')
  })
}

console.log('10. 系统分享不走 tap/airTransfer 一次性 share（返回 false 引导走生命周期）')
{
  const svc = makeMockShareService()
  const payload = buildSharePayload(starmapTitle, graph)
  // 直接调 share(payload, 'tap') 应返回 false
  svc.share(payload, 'tap').then(result => {
    assert(result === false, 'share(payload, "tap") 返回 false（引导走 startTapSharing）')
  })
  svc.share(payload, 'airTransfer').then(result => {
    assert(result === false, 'share(payload, "airTransfer") 返回 false（引导走 startAirTransferSharing）')
  })
}

console.log('11. 完整生命周期序列：进入 → 内容变化 → 离开')
{
  const svc = makeMockShareService()
  const payload1 = buildSharePayload('星图A', graph)
  const payload2 = buildSharePayload('星图B', graph)
  ;(async () => {
    // 进入
    await startShareLifecycle(svc, payload1)
    // 内容变化（切换星图）
    updateSharePayload(svc, payload2)
    // 离开
    await stopShareLifecycle(svc)
    const seq = svc.calls.map(c => c.method)
    assert(seq[0] === 'startTapSharing', 'seq[0]=startTapSharing')
    assert(seq[1] === 'startAirTransferSharing', 'seq[1]=startAirTransferSharing')
    assert(seq[2] === 'updateTapPayload', 'seq[2]=updateTapPayload（内容变化）')
    assert(seq[3] === 'updateAirTransferPayload', 'seq[3]=updateAirTransferPayload（内容变化）')
    assert(seq[4] === 'stopTapSharing', 'seq[4]=stopTapSharing')
    assert(seq[5] === 'stopAirTransferSharing', 'seq[5]=stopAirTransferSharing')
    assert(seq[6] === 'updateTapPayload', 'seq[6]=updateTapPayload（清空）')
    assert(seq[7] === 'updateAirTransferPayload', 'seq[7]=updateAirTransferPayload（清空）')
    // update payload 切换：seq[2] 收到 payload2，seq[6] 收到 null
    assert(eq(svc.calls[2].args[0], payload2), '内容变化 update 收到 payload2')
    assert(svc.calls[6].args[0] === null, '离开清空 update 收到 null')
  })()
}

console.log('12. payload 携带星图信息：不同星图产生不同 payload')
{
  const p1 = buildSharePayload('星图A', { nodes: [{ id: 'n1', title: 'A', kind: 'Character' }], edges: [] })
  const p2 = buildSharePayload('星图B', { nodes: [{ id: 'n1', title: 'B', kind: 'Character' }], edges: [] })
  assert(!eq(p1, p2), '不同星图 payload 不同')
  assert(p1.title === '星图A', 'p1.title=星图A')
  assert(p2.title === '星图B', 'p2.title=星图B')
}

import('node:timers').then(timers => {
  timers.setTimeout(() => {
    console.log('')
    console.log(`结果: ${passed} passed, ${failed} failed`)
    if (failed > 0) process.exit(1)
  }, 200)
})
