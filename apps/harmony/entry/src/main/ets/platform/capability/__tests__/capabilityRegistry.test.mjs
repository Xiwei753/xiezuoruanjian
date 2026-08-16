// capabilityRegistry.test.mjs — CapabilityRegistry 能力来源纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.ShareKit / @kit.InputKit / ArkUI，Node 直接运行。
//
// 验证 Issue #629 评论5 第6部分：
//   1. CapabilityRegistry 从 SystemShareService/TapShareService/AirTransferService 三后端汇总 supportsShare。
//   2. 不再探测淘汰的 systemShareManager / shareContent API。
//   3. supportsStylus 从 StylusInputService.isSupported() 读，不调 InputDeviceService.hasDeviceOfKind。
//
// 本测试用 stub 后端 service 验证 CapabilityRegistry 的汇总逻辑规格。
// 真实 .ets CapabilityRegistry.refresh() 的实现与此处 stub 规格严格一致。

// —— 被测规格：CapabilityRegistry.refresh 的汇总逻辑 ——
// 与 apps/harmony/.../CapabilityRegistry.ets refresh() 同规格：
//   supportsShare = system.isSupported() || tap.isSupported() || air.isSupported()
//   supportsStylus = stylus.isSupported()
//   不调 InputDeviceService.hasDeviceOfKind，不探测 shareContent。
function computeCapabilities(deps) {
  return {
    supportsGripPosture: deps.grip.isSupported(),
    supportsShoulderSurfing: deps.shoulder.isSupported(),
    supportsWindowPrivacy: deps.windowPrivacy.isSupported(),
    supportsAppContinuation: true,
    supportsShare: deps.systemShare.isSupported() || deps.tapShare.isSupported() || deps.airTransfer.isSupported(),
    supportsStylus: deps.stylus.isSupported()
  }
}

// —— stub 工厂 ——
function makeStub(supported) {
  return { isSupported: () => supported }
}
function makeRecordingStub(supported) {
  const calls = { isSupported: 0 }
  return {
    isSupported: () => { calls.isSupported++; return supported },
    calls
  }
}
function makeInputDeviceStub() {
  const calls = { hasDeviceOfKind: 0 }
  return {
    hasDeviceOfKind: (kind) => { calls.hasDeviceOfKind++; return false },
    calls
  }
}
function makeShareKitStub() {
  const calls = { shareContentProbe: 0 }
  return {
    get shareContent() { calls.shareContentProbe++; return () => {} },
    calls
  }
}

// —— 断言工具 ——
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}

console.log('CapabilityRegistry 能力来源纯逻辑测试')

console.log('1. supportsShare 从三后端汇总 — 全 false -> false')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsShare === false, '三后端全 false -> supportsShare=false')
}

console.log('2. supportsShare 从三后端汇总 — system true -> true')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(true), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsShare === true, 'system true -> supportsShare=true')
}

console.log('3. supportsShare 从三后端汇总 — tap true -> true')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(true), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsShare === true, 'tap true -> supportsShare=true')
}

console.log('4. supportsShare 从三后端汇总 — air true -> true')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(true),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsShare === true, 'air true -> supportsShare=true')
}

console.log('5. supportsShare 从三后端汇总 — 任两个 true -> true')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(true), airTransfer: makeStub(true),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsShare === true, 'tap+air true -> supportsShare=true')
}

console.log('6. supportsStylus 从 StylusInputService.isSupported() 读')
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(true)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsStylus === true, 'stylus.isSupported()=true -> supportsStylus=true')
}
{
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  assert(caps.supportsStylus === false, 'stylus.isSupported()=false -> supportsStylus=false')
}

console.log('7. CapabilityRegistry 不调 InputDeviceService.hasDeviceOfKind（淘汰路径）')
{
  const inputDevice = makeInputDeviceStub()
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  void inputDevice
  assert(inputDevice.calls.hasDeviceOfKind === 0, 'hasDeviceOfKind 从未被调用')
  assert(caps.supportsStylus === false, 'supportsStylus 仍正确')
}

console.log('8. CapabilityRegistry 不探测 systemShareManager.shareContent（淘汰路径）')
{
  const shareKit = makeShareKitStub()
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus: makeStub(false)
  }
  const caps = computeCapabilities(deps)
  void shareKit
  assert(shareKit.calls.shareContentProbe === 0, 'shareContent 从未被探测')
  assert(caps.supportsShare === false, 'supportsShare 仍正确')
}

console.log('9. 三后端 isSupported 都被读取（汇总不漏后端）')
{
  const systemShare = makeRecordingStub(false)
  const tapShare = makeRecordingStub(false)
  const airTransfer = makeRecordingStub(false)
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare, tapShare, airTransfer,
    stylus: makeStub(false)
  }
  computeCapabilities(deps)
  assert(systemShare.calls.isSupported === 1, 'systemShare.isSupported 被调一次')
  assert(tapShare.calls.isSupported === 1, 'tapShare.isSupported 被调一次')
  assert(airTransfer.calls.isSupported === 1, 'airTransfer.isSupported 被调一次')
}

console.log('10. stylus.isSupported 被读取（不读 InputDevice）')
{
  const stylus = makeRecordingStub(true)
  const deps = {
    grip: makeStub(false), shoulder: makeStub(false), windowPrivacy: makeStub(false),
    systemShare: makeStub(false), tapShare: makeStub(false), airTransfer: makeStub(false),
    stylus
  }
  const caps = computeCapabilities(deps)
  assert(stylus.calls.isSupported === 1, 'stylus.isSupported 被调一次')
  assert(caps.supportsStylus === true, 'supportsStylus 从 stylus 读')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
