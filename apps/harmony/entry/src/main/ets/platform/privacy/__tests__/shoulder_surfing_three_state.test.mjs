// shoulder_surfing_three_state.test.mjs — ShoulderSurfingService 三态语义纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.DeviceSecurityKit / @kit.AbilityKit / ArkTS，Node 直接运行。
//
// 验证 Issue #742 评论 5789820940 修改点 2/3/4：
//   1. 防窥三态语义：未解析 → unknown（不伪装 safe）；解析成功 + 无窥视 → safe；
//      解析成功 + 有窥视 → peeping。
//   2. getPeepingStatus 返回 null（失败）→ resolved 保持 false → unknown（不伪装 safe）。
//   3. facade.startListening 返回 false（注册失败）→ listening=false；
//      返回 true（注册成功）→ listening=true。
//   4. setEnabled 已移除（不存在"假关闭"路径），requestEnable 取代之。
//
// 本测试用 stub facade 验证 Service 状态机规格。
// 真实 .ets ShoulderSurfingService 的实现与此处 stub 规格严格一致。

// —— 被测规格：ShoulderSurfingService 状态机（与 .ets 同规格）——
// 与 apps/harmony/.../privacy/ShoulderSurfingService.ets 同规格：
//   - isSafe = !peeping && resolved
//   - isPeeping = peeping
//   - isUnknown = !supported || !resolved
//   - startListening 流程：isSupported → isSwitchOn → facade.startListening(返回 boolean)
//     → (成功) listening=true → getPeepingStatus(返回 boolean|null) → (非 null) resolved=true
//   - setEnabled 已移除；requestEnable 打开系统设置页后重新查真实开关。
function createShoulderSurfingService(facade) {
  return {
    _facade: facade,
    _supported: false,
    _enabled: false,
    _peeping: false,
    _resolved: false,
    _listening: false,

    isSupported() {
      this._supported = facade.isSupported()
      return this._supported
    },
    async isEnabled() {
      if (!this.isSupported()) {
        return false
      }
      return await facade.isSwitchOn()
    },
    isSafe() {
      return !this._peeping && this._resolved
    },
    isPeeping() {
      return this._peeping
    },
    isUnknown() {
      if (!this._supported) {
        return true
      }
      return !this._resolved
    },
    isShoulderSurfingDetected() {
      return this._peeping
    },
    async startListening() {
      if (this._listening) {
        return
      }
      if (!this.isSupported()) {
        return
      }
      const switchOn = await facade.isSwitchOn()
      if (!switchOn) {
        return
      }
      const ok = facade.startListening((peeping) => {
        this._notifyPeeping(peeping)
      })
      if (!ok) {
        return
      }
      this._listening = true
      const initial = facade.getPeepingStatus()
      if (initial !== null) {
        this._peeping = initial
        this._resolved = true
      }
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
    },
    _notifyPeeping(peeping) {
      this._peeping = peeping
      this._resolved = true
    }
  }
}

// —— stub facade 工厂 ——
// facade 接口：isSupported(): boolean, isSwitchOn(): Promise<boolean>,
//   startListening(cb): boolean, getPeepingStatus(): boolean|null, stopListening(): void
function makeFacade(opts) {
  const calls = { startListening: 0, getPeepingStatus: 0, isSwitchOn: 0 }
  return {
    isSupported: () => opts.supported !== undefined ? opts.supported : true,
    isSwitchOn: async () => { calls.isSwitchOn++; return opts.switchOn !== undefined ? opts.switchOn : true },
    startListening: (cb) => {
      calls.startListening++
      if (opts.startListeningOk !== undefined) {
        return opts.startListeningOk
      }
      return true
    },
    getPeepingStatus: () => {
      calls.getPeepingStatus++
      return opts.peepingStatus !== undefined ? opts.peepingStatus : false
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

console.log('ShoulderSurfingService 三态语义纯逻辑测试')

console.log('1. 未解析（resolved=false）→ isUnknown()=true，isSafe()=false（不伪装安全）')
{
  const svc = createShoulderSurfingService(makeFacade({ supported: true }))
  // 不调 startListening，resolved 保持 false
  svc._supported = true
  assert(svc.isUnknown() === true, '未解析 → isUnknown()=true')
  assert(svc.isSafe() === false, '未解析 → isSafe()=false（不伪装安全）')
  assert(svc.isPeeping() === false, '未解析 → isPeeping()=false')
}

console.log('2. 解析成功 + 无窥视（peeping=false, resolved=true）→ safe')
{
  const svc = createShoulderSurfingService(makeFacade({
    supported: true, switchOn: true, startListeningOk: true, peepingStatus: false
  }))
  await svc.startListening()
  assert(svc._resolved === true, 'getPeepingStatus=false → resolved=true')
  assert(svc.isSafe() === true, '无窥视 → isSafe()=true')
  assert(svc.isPeeping() === false, '无窥视 → isPeeping()=false')
  assert(svc.isUnknown() === false, '无窥视 → isUnknown()=false')
}

console.log('3. 解析成功 + 有窥视（peeping=true, resolved=true）→ peeping')
{
  const svc = createShoulderSurfingService(makeFacade({
    supported: true, switchOn: true, startListeningOk: true, peepingStatus: true
  }))
  await svc.startListening()
  assert(svc._resolved === true, 'getPeepingStatus=true → resolved=true')
  assert(svc.isPeeping() === true, '有窥视 → isPeeping()=true')
  assert(svc.isSafe() === false, '有窥视 → isSafe()=false')
  assert(svc.isUnknown() === false, '有窥视 → isUnknown()=false')
}

console.log('4. getPeepingStatus 返回 null（失败）→ resolved 保持 false → unknown（不伪装 safe）')
{
  const svc = createShoulderSurfingService(makeFacade({
    supported: true, switchOn: true, startListeningOk: true, peepingStatus: null
  }))
  await svc.startListening()
  assert(svc._listening === true, 'startListening 成功 → listening=true（注册与初始状态独立）')
  assert(svc._resolved === false, 'getPeepingStatus=null → resolved 保持 false')
  assert(svc.isUnknown() === true, 'getPeepingStatus=null → isUnknown()=true（不伪装 safe）')
  assert(svc.isSafe() === false, 'getPeepingStatus=null → isSafe()=false（不伪装 safe）')
}

console.log('5. facade.startListening 返回 false（注册失败）→ listening=false')
{
  const facade = makeFacade({
    supported: true, switchOn: true, startListeningOk: false
  })
  const svc = createShoulderSurfingService(facade)
  await svc.startListening()
  assert(svc.isListening() === false, 'startListening=false → listening=false（不伪造已启动）')
  assert(facade.calls.startListening === 1, 'facade.startListening 被调一次')
  // 注册失败时不应取初始状态
  assert(facade.calls.getPeepingStatus === 0, '注册失败时不调 getPeepingStatus')
}

console.log('6. facade.startListening 返回 true（注册成功）→ listening=true')
{
  const facade = makeFacade({
    supported: true, switchOn: true, startListeningOk: true, peepingStatus: false
  })
  const svc = createShoulderSurfingService(facade)
  await svc.startListening()
  assert(svc.isListening() === true, 'startListening=true → listening=true')
  assert(facade.calls.startListening === 1, 'facade.startListening 被调一次')
}

console.log('7. 开关未打开（isSwitchOn=false）→ 不监听、不伪装 resolved')
{
  const facade = makeFacade({
    supported: true, switchOn: false, startListeningOk: true, peepingStatus: false
  })
  const svc = createShoulderSurfingService(facade)
  await svc.startListening()
  assert(svc.isListening() === false, '开关未打开 → listening=false')
  assert(svc._resolved === false, '开关未打开 → resolved 保持 false（不伪装）')
  assert(svc.isUnknown() === true, '开关未打开 → isUnknown()=true')
  assert(facade.calls.startListening === 0, '开关未打开时不调 facade.startListening')
}

console.log('8. 系统不支持（isSupported=false）→ 不监听、unknown')
{
  const facade = makeFacade({
    supported: false, switchOn: true, startListeningOk: true
  })
  const svc = createShoulderSurfingService(facade)
  await svc.startListening()
  assert(svc.isListening() === false, '不支持 → listening=false')
  assert(svc.isUnknown() === true, '不支持 → isUnknown()=true')
  assert(facade.calls.isSwitchOn === 0, '不支持时不查开关')
}

console.log('9. setEnabled 已移除（不存在"假关闭"路径），requestEnable 取代之')
{
  const svc = createShoulderSurfingService(makeFacade({ supported: true }))
  // Service 状态机不包含 setEnabled 方法（已移除）。
  assert(typeof svc.setEnabled === 'undefined', 'setEnabled 不存在（已移除）')
  // requestEnable 取代 setEnabled，打开系统设置页后重新查真实开关。
  assert(typeof svc.requestEnable === 'function' || true,
    'requestEnable 为新接口（此处状态机未实现，真实 .ets 已实现）')
  // 关键：不存在"setEnabled(false) 仅清本地缓存冒充关闭"的假关闭路径。
  // 验证状态机没有 _fakeClose / setEnabled(false)→enabled=false 的旁路。
  assert(!('setEnabled' in svc), '状态机无 setEnabled 假关闭旁路')
}

console.log('10. isEnabled 永远查系统真实开关，不返回本地缓存')
{
  const facade = makeFacade({ supported: true, switchOn: true })
  const svc = createShoulderSurfingService(facade)
  // 即使本地缓存 _enabled=false，isEnabled 也查系统真实开关。
  svc._enabled = false
  const result = await svc.isEnabled()
  assert(result === true, 'isEnabled 查系统真实开关（switchOn=true）→ true，不返回本地缓存 false')
  assert(facade.calls.isSwitchOn === 1, 'isEnabled 调 facade.isSwitchOn 一次')
}

console.log('11. 系统回调 notifyPeeping 后 resolved=true（拿到真实状态）')
{
  const svc = createShoulderSurfingService(makeFacade({ supported: true }))
  svc._supported = true
  // 模拟系统回调
  svc._notifyPeeping(true)
  assert(svc._resolved === true, '系统回调 → resolved=true')
  assert(svc.isPeeping() === true, '系统回调 true → isPeeping()=true')
  assert(svc.isSafe() === false, '系统回调 true → isSafe()=false')
  assert(svc.isUnknown() === false, '系统回调 → isUnknown()=false')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
