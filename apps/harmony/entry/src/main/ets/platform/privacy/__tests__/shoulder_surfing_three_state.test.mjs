// shoulder_surfing_three_state.test.mjs — ShoulderSurfingService 三态语义纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.DeviceSecurityKit / @kit.AbilityKit / ArkTS，Node 直接运行。
//
// 验证 Issue #742 评论 5789820940 修改点 2/3/4：
//   1. 防窥三态语义：未解析 → unknown（不伪装 safe）；解析成功 + 无窥视 → safe；
//      解析成功 + 有窥视 → peeping。
//   2. getPeepingStatus 返回 null（失败）→ resolved 保持 false → unknown（不伪装 safe）。
//   3. provider.startListening 返回 false（注册失败）→ listening=false；
//      返回 true（注册成功）→ listening=true。
//   4. setEnabled 已移除（不存在"假关闭"路径），requestEnable 取代之。
//
// Issue #914：DLP 实现迁出 entry 后，ShoulderSurfingService 不再直接调 DlpAntiPeepApi20，
// 而是委托 ShoulderSurfingPort，由 target 的 ShoulderSurfingProvider 实现：
//   - default target → standard Provider（isSupported 返回 false，不引用 DLP）；
//   - acl target → acl Provider（委托 privacy_acl HSP 的真实 DLP 实现）。
// Port 的开关查询方法名是 isEnabled()（不再是旧 facade 的 isSwitchOn()），并含
// setAbilityContext(context)。本测试的 stub 与当前 Port/Service 契约严格对齐。
//
// 本测试用 stub provider 验证 Service 状态机规格。
// 真实 .ets ShoulderSurfingService 的实现与此处 stub 规格严格一致。

// —— 被测规格：ShoulderSurfingService 状态机（与 .ets 同规格）——
// 与 apps/harmony/.../privacy/ShoulderSurfingService.ets 同规格：
//   - isSafe = !peeping && resolved
//   - isPeeping = peeping
//   - isUnknown = !supported || !resolved
//   - setAbilityContext 转传给 Port（acl Provider 保存供 requestAntiPeepOptions 用）
//   - startListening 流程：isSupported → provider.isEnabled → provider.startListening(返回 boolean)
//     → (成功) listening=true → provider.getPeepingStatus(返回 boolean|null) → (非 null) resolved=true
//   - setEnabled 已移除；requestEnable 打开系统设置页后重新查真实开关。
function createShoulderSurfingService(provider) {
  return {
    _provider: provider,
    _supported: false,
    _enabled: false,
    _peeping: false,
    _resolved: false,
    _listening: false,

    setAbilityContext(context) {
      this._provider.setAbilityContext(context)
    },
    isSupported() {
      this._supported = this._provider.isSupported()
      return this._supported
    },
    async isEnabled() {
      if (!this.isSupported()) {
        return false
      }
      return await this._provider.isEnabled()
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
    async requestEnable() {
      if (!this.isSupported()) {
        this._enabled = false
        return false
      }
      this._enabled = await this._provider.requestEnable()
      return this._enabled
    },
    async startListening() {
      if (this._listening) {
        return
      }
      if (!this.isSupported()) {
        return
      }
      const switchOn = await this._provider.isEnabled()
      if (!switchOn) {
        return
      }
      const ok = this._provider.startListening((peeping) => {
        this._notifyPeeping(peeping)
      })
      if (!ok) {
        return
      }
      this._listening = true
      const initial = this._provider.getPeepingStatus()
      if (initial !== null) {
        this._peeping = initial
        this._resolved = true
      }
    },
    stopListening() {
      if (!this._listening) {
        return
      }
      this._provider.stopListening()
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

// —— stub provider（ShoulderSurfingPort）工厂 ——
// Port 接口：setAbilityContext(ctx): void, isSupported(): boolean,
//   isEnabled(): Promise<boolean>, requestEnable(): Promise<boolean>,
//   startListening(cb): boolean, getPeepingStatus(): boolean|null, stopListening(): void
function makeProvider(opts) {
  const calls = { startListening: 0, getPeepingStatus: 0, isEnabled: 0, requestEnable: 0, setAbilityContext: 0 }
  return {
    setAbilityContext: () => { calls.setAbilityContext++ },
    isSupported: () => opts.supported !== undefined ? opts.supported : true,
    isEnabled: async () => { calls.isEnabled++; return opts.enabled !== undefined ? opts.enabled : true },
    requestEnable: async () => {
      calls.requestEnable++
      return opts.requestEnableResult !== undefined ? opts.requestEnableResult : true
    },
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
  const svc = createShoulderSurfingService(makeProvider({ supported: true }))
  // 不调 startListening，resolved 保持 false
  svc._supported = true
  assert(svc.isUnknown() === true, '未解析 → isUnknown()=true')
  assert(svc.isSafe() === false, '未解析 → isSafe()=false（不伪装安全）')
  assert(svc.isPeeping() === false, '未解析 → isPeeping()=false')
}

console.log('2. 解析成功 + 无窥视（peeping=false, resolved=true）→ safe')
{
  const svc = createShoulderSurfingService(makeProvider({
    supported: true, enabled: true, startListeningOk: true, peepingStatus: false
  }))
  await svc.startListening()
  assert(svc._resolved === true, 'getPeepingStatus=false → resolved=true')
  assert(svc.isSafe() === true, '无窥视 → isSafe()=true')
  assert(svc.isPeeping() === false, '无窥视 → isPeeping()=false')
  assert(svc.isUnknown() === false, '无窥视 → isUnknown()=false')
}

console.log('3. 解析成功 + 有窥视（peeping=true, resolved=true）→ peeping')
{
  const svc = createShoulderSurfingService(makeProvider({
    supported: true, enabled: true, startListeningOk: true, peepingStatus: true
  }))
  await svc.startListening()
  assert(svc._resolved === true, 'getPeepingStatus=true → resolved=true')
  assert(svc.isPeeping() === true, '有窥视 → isPeeping()=true')
  assert(svc.isSafe() === false, '有窥视 → isSafe()=false')
  assert(svc.isUnknown() === false, '有窥视 → isUnknown()=false')
}

console.log('4. getPeepingStatus 返回 null（失败）→ resolved 保持 false → unknown（不伪装 safe）')
{
  const svc = createShoulderSurfingService(makeProvider({
    supported: true, enabled: true, startListeningOk: true, peepingStatus: null
  }))
  await svc.startListening()
  assert(svc._listening === true, 'startListening 成功 → listening=true（注册与初始状态独立）')
  assert(svc._resolved === false, 'getPeepingStatus=null → resolved 保持 false')
  assert(svc.isUnknown() === true, 'getPeepingStatus=null → isUnknown()=true（不伪装 safe）')
  assert(svc.isSafe() === false, 'getPeepingStatus=null → isSafe()=false（不伪装 safe）')
}

console.log('5. provider.startListening 返回 false（注册失败）→ listening=false')
{
  const provider = makeProvider({
    supported: true, enabled: true, startListeningOk: false
  })
  const svc = createShoulderSurfingService(provider)
  await svc.startListening()
  assert(svc.isListening() === false, 'startListening=false → listening=false（不伪造已启动）')
  assert(provider.calls.startListening === 1, 'provider.startListening 被调一次')
  // 注册失败时不应取初始状态
  assert(provider.calls.getPeepingStatus === 0, '注册失败时不调 getPeepingStatus')
}

console.log('6. provider.startListening 返回 true（注册成功）→ listening=true')
{
  const provider = makeProvider({
    supported: true, enabled: true, startListeningOk: true, peepingStatus: false
  })
  const svc = createShoulderSurfingService(provider)
  await svc.startListening()
  assert(svc.isListening() === true, 'startListening=true → listening=true')
  assert(provider.calls.startListening === 1, 'provider.startListening 被调一次')
}

console.log('7. 开关未打开（provider.isEnabled=false）→ 不监听、不伪装 resolved')
{
  const provider = makeProvider({
    supported: true, enabled: false, startListeningOk: true, peepingStatus: false
  })
  const svc = createShoulderSurfingService(provider)
  await svc.startListening()
  assert(svc.isListening() === false, '开关未打开 → listening=false')
  assert(svc._resolved === false, '开关未打开 → resolved 保持 false（不伪装）')
  assert(svc.isUnknown() === true, '开关未打开 → isUnknown()=true')
  assert(provider.calls.startListening === 0, '开关未打开时不调 provider.startListening')
}

console.log('8. 系统不支持（isSupported=false）→ 不监听、unknown')
{
  const provider = makeProvider({
    supported: false, enabled: true, startListeningOk: true
  })
  const svc = createShoulderSurfingService(provider)
  await svc.startListening()
  assert(svc.isListening() === false, '不支持 → listening=false')
  assert(svc.isUnknown() === true, '不支持 → isUnknown()=true')
  assert(provider.calls.isEnabled === 0, '不支持时不查开关')
}

console.log('9. setEnabled 已移除（不存在"假关闭"路径），requestEnable 取代之')
{
  const provider = makeProvider({ supported: true })
  const svc = createShoulderSurfingService(provider)
  // Service 状态机不包含 setEnabled 方法（已移除）。
  assert(typeof svc.setEnabled === 'undefined', 'setEnabled 不存在（已移除）')
  // requestEnable 取代 setEnabled，打开系统设置页后重新查真实开关。
  assert(typeof svc.requestEnable === 'function', 'requestEnable 为新接口')
  // 关键：不存在"setEnabled(false) 仅清本地缓存冒充关闭"的假关闭路径。
  assert(!('setEnabled' in svc), '状态机无 setEnabled 假关闭旁路')
  // 不支持时 requestEnable 返回 false，且不调 provider.requestEnable。
  const unsupported = createShoulderSurfingService(makeProvider({ supported: false }))
  const denied = await unsupported.requestEnable()
  assert(denied === false, '不支持 → requestEnable()=false，不伪造')
  // 支持时委托 provider.requestEnable 的结果。
  const enabled = await svc.requestEnable()
  assert(enabled === true, '支持 → requestEnable() 委托 provider 返回 true')
  assert(provider.calls.requestEnable === 1, 'requestEnable 调 provider.requestEnable 一次')
}

console.log('10. isEnabled 永远查系统真实开关，不返回本地缓存')
{
  const provider = makeProvider({ supported: true, enabled: true })
  const svc = createShoulderSurfingService(provider)
  // 即使本地缓存 _enabled=false，isEnabled 也查系统真实开关。
  svc._enabled = false
  const result = await svc.isEnabled()
  assert(result === true, 'isEnabled 查系统真实开关（enabled=true）→ true，不返回本地缓存 false')
  assert(provider.calls.isEnabled === 1, 'isEnabled 调 provider.isEnabled 一次')
}

console.log('11. 系统回调 notifyPeeping 后 resolved=true（拿到真实状态）')
{
  const svc = createShoulderSurfingService(makeProvider({ supported: true }))
  svc._supported = true
  // 模拟系统回调
  svc._notifyPeeping(true)
  assert(svc._resolved === true, '系统回调 → resolved=true')
  assert(svc.isPeeping() === true, '系统回调 true → isPeeping()=true')
  assert(svc.isSafe() === false, '系统回调 true → isSafe()=false')
  assert(svc.isUnknown() === false, '系统回调 → isUnknown()=false')
}

console.log('12. setAbilityContext 转传给 Port（acl Provider 保存 context）')
{
  const provider = makeProvider({ supported: true })
  const svc = createShoulderSurfingService(provider)
  svc.setAbilityContext({ uiContext: 'fake' })
  assert(provider.calls.setAbilityContext === 1, 'setAbilityContext 转传给 provider 一次')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
