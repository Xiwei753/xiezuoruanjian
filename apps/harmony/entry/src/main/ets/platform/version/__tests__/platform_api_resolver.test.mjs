// platform_api_resolver.test.mjs — PlatformApiResolver 版本判断纯逻辑测试。
// 纯 JS（.mjs），不依赖 @kit.BasicServicesKit / ArkTS，Node 直接运行。
//
// 验证 Issue #742 评论 5789820940 修改点 1：
//   1. PlatformApiResolver 改用 @kit.BasicServicesKit 的 deviceInfo.sdkApiVersion（非 sdkVersion）。
//   2. api20 isSupported = isApiAtLeast(20) && hasSystemCapability(...)（两者都 true 才 true）。
//   3. 读取失败 → getApiLevel()=0 → isApiAtLeast(20)=false。
//
// 本测试用 stub deviceInfo.sdkApiVersion 验证 PlatformApiResolver 版本判断规格。
// 真实 .ets PlatformApiResolver 的实现与此处 stub 规格严格一致。

// —— 被测规格：PlatformApiResolver 状态机（与 .ets 同规格）——
// 与 apps/harmony/.../version/PlatformApiResolver.ets 同规格：
//   - getApiLevel(): 缓存；读 deviceInfo.sdkApiVersion；失败返回 0 且不缓存。
//   - canIUse(syscap): try { return canIUse(syscap) } catch { return false }。
//   - hasSystemCapability = canIUse。
//   - isApiAtLeast(level) = getApiLevel() >= level。
function createResolver(deviceInfo, canIUseFn) {
  return {
    _apiLevel: 0,
    _levelResolved: false,
    _deviceInfo: deviceInfo,
    _canIUse: canIUseFn,

    canIUse(syscap) {
      try {
        return this._canIUse(syscap)
      } catch (e) {
        return false
      }
    },
    getApiLevel() {
      if (this._levelResolved) {
        return this._apiLevel
      }
      try {
        this._apiLevel = this._deviceInfo.sdkApiVersion
        this._levelResolved = true
      } catch (e) {
        this._apiLevel = 0
        this._levelResolved = false
      }
      return this._apiLevel
    },
    hasSystemCapability(syscap) {
      return this.canIUse(syscap)
    },
    isApiAtLeast(level) {
      return this.getApiLevel() >= level
    }
  }
}

// —— api20 isSupported 规格（与 DlpAntiPeepApi20.ets / GripPostureApi20.ets 同规格）——
// isSupported = isApiAtLeast(20) && hasSystemCapability(syscap)
function api20IsSupported(resolver, syscap) {
  return resolver.isApiAtLeast(20) && resolver.hasSystemCapability(syscap)
}

// —— stub 工厂 ——
function makeDeviceInfo(sdkApiVersion) {
  return { sdkApiVersion }
}
// 模拟读取失败：sdkApiVersion getter 抛异常
function makeFailingDeviceInfo() {
  return {
    get sdkApiVersion() { throw new Error('read failed') }
  }
}
function makeCanIUse(capabilityMap) {
  return (syscap) => capabilityMap[syscap] === true
}

// —— 断言工具 ——
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}

console.log('PlatformApiResolver 版本判断纯逻辑测试')

console.log('1. sdkApiVersion=20 → isApiAtLeast(20)=true, isApiAtLeast(21)=false')
{
  const r = createResolver(makeDeviceInfo(20), makeCanIUse({}))
  assert(r.getApiLevel() === 20, 'getApiLevel()=20')
  assert(r.isApiAtLeast(20) === true, 'isApiAtLeast(20)=true')
  assert(r.isApiAtLeast(21) === false, 'isApiAtLeast(21)=false')
  assert(r.isApiAtLeast(19) === true, 'isApiAtLeast(19)=true')
}

console.log('2. sdkApiVersion=23 → isApiAtLeast(23)=true')
{
  const r = createResolver(makeDeviceInfo(23), makeCanIUse({}))
  assert(r.getApiLevel() === 23, 'getApiLevel()=23')
  assert(r.isApiAtLeast(23) === true, 'isApiAtLeast(23)=true')
  assert(r.isApiAtLeast(24) === false, 'isApiAtLeast(24)=false')
  assert(r.isApiAtLeast(20) === true, 'isApiAtLeast(20)=true')
}

console.log('3. 读取失败 → getApiLevel()=0 → isApiAtLeast(20)=false')
{
  const r = createResolver(makeFailingDeviceInfo(), makeCanIUse({}))
  assert(r.getApiLevel() === 0, '读取失败 → getApiLevel()=0')
  assert(r.isApiAtLeast(20) === false, '读取失败 → isApiAtLeast(20)=false')
  assert(r.isApiAtLeast(0) === true, '读取失败 → isApiAtLeast(0)=true（0>=0）')
}

console.log('4. api20 isSupported = isApiAtLeast(20) && hasSystemCapability（两者都 true 才 true）')
{
  const r = createResolver(makeDeviceInfo(20), makeCanIUse({
    'SystemCapability.Security.DlpAntiPeep': true
  }))
  assert(api20IsSupported(r, 'SystemCapability.Security.DlpAntiPeep') === true,
    'api20+capability 都 true → isSupported=true')
}

console.log('5. api20 isSupported：API>=20 但 SystemCapability=false → false')
{
  const r = createResolver(makeDeviceInfo(20), makeCanIUse({
    'SystemCapability.Security.DlpAntiPeep': false
  }))
  assert(api20IsSupported(r, 'SystemCapability.Security.DlpAntiPeep') === false,
    'api20=true + capability=false → isSupported=false')
}

console.log('6. api20 isSupported：SystemCapability=true 但 API<20 → false')
{
  const r = createResolver(makeDeviceInfo(19), makeCanIUse({
    'SystemCapability.Security.DlpAntiPeep': true
  }))
  assert(api20IsSupported(r, 'SystemCapability.Security.DlpAntiPeep') === false,
    'api19 + capability=true → isSupported=false（API 不够）')
}

console.log('7. api20 isSupported：读取失败（API=0）+ capability=true → false')
{
  const r = createResolver(makeFailingDeviceInfo(), makeCanIUse({
    'SystemCapability.Security.DlpAntiPeep': true
  }))
  assert(api20IsSupported(r, 'SystemCapability.Security.DlpAntiPeep') === false,
    '读取失败 + capability=true → isSupported=false（API=0 不够）')
}

console.log('8. GripPostureApi20 isSupported = isApiAtLeast(20) && hasSystemCapability(Motion)')
{
  const r = createResolver(makeDeviceInfo(20), makeCanIUse({
    'SystemCapability.MultimodalAwareness.Motion': true
  }))
  assert(api20IsSupported(r, 'SystemCapability.MultimodalAwareness.Motion') === true,
    'api20 + Motion capability → isSupported=true')
}
{
  const r = createResolver(makeDeviceInfo(20), makeCanIUse({
    'SystemCapability.MultimodalAwareness.Motion': false
  }))
  assert(api20IsSupported(r, 'SystemCapability.MultimodalAwareness.Motion') === false,
    'api20 + Motion capability=false → isSupported=false')
}

console.log('9. getApiLevel 结果缓存（不重复读 deviceInfo）')
{
  let readCount = 0
  const deviceInfo = {
    get sdkApiVersion() { readCount++; return 20 }
  }
  const r = createResolver(deviceInfo, makeCanIUse({}))
  r.getApiLevel()
  r.getApiLevel()
  r.getApiLevel()
  assert(readCount === 1, '多次 getApiLevel 只读 deviceInfo 一次（缓存）')
}

console.log('10. canIUse 抛异常 → hasSystemCapability 返回 false（不伪造 true）')
{
  const r = createResolver(makeDeviceInfo(20), () => { throw new Error('canIUse failed') })
  assert(r.hasSystemCapability('SystemCapability.Security.DlpAntiPeep') === false,
    'canIUse 抛异常 → hasSystemCapability=false')
  assert(r.canIUse('any') === false, 'canIUse 抛异常 → canIUse=false')
}

console.log('11. 读取失败后不缓存，下次重试（levelResolved 保持 false）')
{
  let fail = true
  const deviceInfo = {
    get sdkApiVersion() { if (fail) throw new Error('fail'); return 22 }
  }
  const r = createResolver(deviceInfo, makeCanIUse({}))
  assert(r.getApiLevel() === 0, '第一次读取失败 → 0')
  fail = false
  assert(r.getApiLevel() === 22, '第二次读取成功 → 22（未缓存失败，可重试）')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
