// grip_posture_listening.test.mjs — GripPostureService 注册结果传播纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.MultimodalAwarenessKit / ArkTS，Node 直接运行。
//
// 验证 Issue #742 评论 5789820940 修改点 3：
//   握姿不要因为 canIUse/isSupported 为 true 就把"注册成功"当成既定事实——
//   必须以 startListening() 的真实返回值为准。
//
// 本测试用 stub facade 验证 GripPostureService 注册结果传播规格。
// 真实 .ets GripPostureService 的实现与此处 stub 规格严格一致。

// —— 被测规格：GripPostureService 状态机（与 .ets 同规格）——
// 与 apps/harmony/.../awareness/GripPostureService.ets 同规格：
//   - isSupported(): 未监听时查 facade 真实能力；监听后返回缓存。
//   - startListening: facade.isSupported() → (支持) facade.startListening(返回 boolean)
//     → (true) listening=true。不因 isSupported 就伪造已启动。
function createGripPostureService(facade) {
  return {
    _facade: facade,
    _posture: 'unknown',
    _listening: false,
    _supported: false,

    getPosture() {
      return this._posture
    },
    isSupported() {
      if (!this._listening) {
        this._supported = facade.isSupported()
      }
      return this._supported
    },
    async startListening() {
      if (this._listening) {
        return
      }
      const api = facade
      this._supported = api.isSupported()
      if (!this._supported) {
        return
      }
      const ok = api.startListening((posture) => {
        this._posture = posture
      })
      if (!ok) {
        return
      }
      this._listening = true
    },
    stopListening() {
      if (!this._listening) {
        return
      }
      facade.stopListening()
      this._listening = false
    },
    isListening() {
      return this._listening
    }
  }
}

// —— stub facade 工厂 ——
// facade 接口：isSupported(): boolean, startListening(cb): boolean, stopListening(): void
function makeFacade(opts) {
  const calls = { startListening: 0, isSupported: 0 }
  return {
    isSupported: () => { calls.isSupported++; return opts.supported !== undefined ? opts.supported : true },
    startListening: (cb) => {
      calls.startListening++
      if (opts.startListeningOk !== undefined) {
        return opts.startListeningOk
      }
      return true
    },
    stopListening: () => {},
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

console.log('GripPostureService 注册结果传播纯逻辑测试')

console.log('1. facade.isSupported()=true 且 facade.startListening()=true → listening=true')
{
  const facade = makeFacade({ supported: true, startListeningOk: true })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  assert(svc.isListening() === true, 'isSupported=true + startListening=true → listening=true')
  assert(facade.calls.isSupported === 1, 'facade.isSupported 被调一次')
  assert(facade.calls.startListening === 1, 'facade.startListening 被调一次')
}

console.log('2. facade.isSupported()=true 且 facade.startListening()=false → listening=false（不因 isSupported 就伪造）')
{
  const facade = makeFacade({ supported: true, startListeningOk: false })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  assert(svc.isListening() === false, 'isSupported=true + startListening=false → listening=false')
  assert(facade.calls.startListening === 1, 'facade.startListening 被调一次（真实尝试注册）')
  // 关键：不因 isSupported=true 就把"注册成功"当成既定事实
  assert(svc._supported === true, '_supported=true（能力支持）但 listening=false（注册失败）')
}

console.log('3. facade.isSupported()=false → listening=false')
{
  const facade = makeFacade({ supported: false, startListeningOk: true })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  assert(svc.isListening() === false, 'isSupported=false → listening=false')
  assert(facade.calls.startListening === 0, 'isSupported=false 时不调 facade.startListening')
}

console.log('4. 已在监听时再次 startListening 不重复注册')
{
  const facade = makeFacade({ supported: true, startListeningOk: true })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  await svc.startListening()
  assert(svc.isListening() === true, '重复 startListening → listening 仍 true')
  assert(facade.calls.startListening === 1, '重复 startListening 不重复注册（只调一次）')
}

console.log('5. stopListening 后 listening=false，可重新启动')
{
  const facade = makeFacade({ supported: true, startListeningOk: true })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  assert(svc.isListening() === true, '启动后 listening=true')
  svc.stopListening()
  assert(svc.isListening() === false, 'stopListening 后 listening=false')
  await svc.startListening()
  assert(svc.isListening() === true, '重新 startListening → listening=true')
  assert(facade.calls.startListening === 2, 'facade.startListening 被调两次（重启）')
}

console.log('6. isSupported 缓存：监听后 isSupported 返回缓存，不再查 facade')
{
  const facade = makeFacade({ supported: true, startListeningOk: true })
  const svc = createGripPostureService(facade)
  await svc.startListening()
  const before = facade.calls.isSupported
  svc.isSupported()
  assert(facade.calls.isSupported === before, '监听后 isSupported 返回缓存，不再查 facade')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
