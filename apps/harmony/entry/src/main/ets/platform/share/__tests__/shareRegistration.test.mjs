// shareRegistration.test.mjs - AirTransfer/TapShare start real registration result test.
// Pure JS (.mjs), no @kit.ShareKit / ArkUI dependency, run with node.
//
// Issue #629 comment5 part7: start returns real registration success/failure, not always true.

// TapShareService.start spec — 委托 KnockShareApi12 facade。
// Service 不直接调 harmonyShare；经 impl facade 的 isSupported + startListening。
// 边界：稳定 Service 不 import @kit.ShareKit，系统调用收在 impl/api12。
// knockImpl: { isSupported(): boolean, startListening(getPayload): boolean, stopListening(): void }
function makeTapShare(knockImpl) {
  return {
    listening: false,
    supported: false,
    currentPayload: null,
    start() {
      if (this.listening) return true
      if (!knockImpl.isSupported()) {
        this.listening = false
        this.supported = false
        return false
      }
      const ok = knockImpl.startListening(() => this.currentPayload)
      if (ok) {
        this.listening = true
        this.supported = true
        return true
      }
      this.listening = false
      this.supported = false
      return false
    },
    isSupported() {
      if (!this.listening) this.supported = knockImpl.isSupported()
      return this.supported
    }
  }
}

// 默认 KnockShareApi12 stub：isSupported=true，startListening 调 harmonyShare.on 返回真实结果。
// harmonyShare.on('knockShare', cb) 是无 capability 基础重载（since 5.0.0(12)）。
function makeKnockImpl(harmonyShare, opts) {
  const supported = opts && opts.supported !== undefined ? opts.supported : true
  return {
    isSupported() { return supported },
    startListening(getPayload) {
      try {
        harmonyShare.on('knockShare', () => {})
        return true
      } catch (e) {
        return false
      }
    },
    stopListening() {
      try { harmonyShare.off('knockShare') } catch (e) {}
    }
  }
}

// AirTransferService.start spec
function makeAirTransfer(harmonyShare) {
  return {
    listening: false,
    supported: false,
    activeWindowId: -1,
    start(windowId) {
      if (this.listening) {
        if (this.activeWindowId === windowId) return true
        this.stopInternal()
      }
      try {
        harmonyShare.on('gesturesShare', { windowId }, () => {})
        this.listening = true
        this.activeWindowId = windowId
        this.supported = true
        return true
      } catch (e) {
        this.listening = false
        this.activeWindowId = -1
        this.supported = false
        return false
      }
    },
    stopInternal() {
      try { harmonyShare.off('gesturesShare') } catch (e) {}
      this.listening = false
      this.activeWindowId = -1
    },
    isSupported() { return this.supported }
  }
}

// HarmonyShareService.startAirTransferSharing spec: returns real result, not fake true
async function startAirTransferSharing(airTransfer, getWindowId, payload, windowId) {
  let wid
  if (windowId !== undefined && windowId !== null) {
    wid = windowId
  } else {
    const resolved = await getWindowId()
    if (resolved === null) return false
    wid = resolved
  }
  return airTransfer.start(wid)
}

// stub harmonyShare
function makeHarmonyShare(shouldThrow) {
  const calls = { on: 0, off: 0 }
  return {
    on(event, opt, cb) {
      calls.on++
      if (shouldThrow) throw new Error('mock: API unavailable')
    },
    off(event) { calls.off++ },
    calls
  }
}

// assert utils
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}

console.log('AirTransfer/TapShare real registration result test')

console.log('1. TapShareService.start success returns true')
{
  const hs = makeHarmonyShare(false)
  const tap = makeTapShare(makeKnockImpl(hs))
  const ok = tap.start()
  assert(ok === true, 'start returns true')
  assert(tap.listening === true, 'listening=true')
  assert(tap.isSupported() === true, 'isSupported=true')
}

console.log('2. TapShareService.start failure returns false (not always true)')
{
  const hs = makeHarmonyShare(true)
  const tap = makeTapShare(makeKnockImpl(hs))
  const ok = tap.start()
  assert(ok === false, 'start returns false on failure')
  assert(tap.listening === false, 'listening=false')
  // isSupported() 未监听时查询 facade 真实能力（默认 true）；不因注册失败伪造为 false。
  // 注册失败由 start 返回 false 和 listening=false 体现，不污染能力查询。
  assert(tap.isSupported() === true, 'isSupported reflects facade capability, not registration result')
}

console.log('3. TapShareService.start idempotent: already listening returns true')
{
  const hs = makeHarmonyShare(false)
  const tap = makeTapShare(makeKnockImpl(hs))
  tap.start()
  const ok = tap.start()
  assert(ok === true, 'second start returns true')
  assert(hs.calls.on === 1, 'harmonyShare.on called once')
}

console.log('3b. TapShareService.start unsupported: isSupported=false -> false, no registration')
{
  const hs = makeHarmonyShare(false)
  const tap = makeTapShare(makeKnockImpl(hs, { supported: false }))
  const ok = tap.start()
  assert(ok === false, 'unsupported -> start returns false')
  assert(tap.listening === false, 'listening=false')
  assert(hs.calls.on === 0, 'harmonyShare.on not called when unsupported')
}

console.log('4. AirTransferService.start success returns true')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  const ok = air.start(42)
  assert(ok === true, 'start returns true')
  assert(air.listening === true, 'listening=true')
  assert(air.activeWindowId === 42, 'activeWindowId=42')
  assert(air.isSupported() === true, 'isSupported=true')
}

console.log('5. AirTransferService.start failure returns false (not always true)')
{
  const hs = makeHarmonyShare(true)
  const air = makeAirTransfer(hs)
  const ok = air.start(42)
  assert(ok === false, 'start returns false on failure')
  assert(air.listening === false, 'listening=false')
  assert(air.isSupported() === false, 'isSupported=false')
}

console.log('6. AirTransferService.start idempotent: same windowId returns true')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  air.start(42)
  const ok = air.start(42)
  assert(ok === true, 'same windowId second start returns true')
  assert(hs.calls.on === 1, 'harmonyShare.on called once')
}

console.log('7. AirTransferService.start change windowId: stop then start')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  air.start(42)
  const ok = air.start(99)
  assert(ok === true, 'change windowId start returns true')
  assert(air.activeWindowId === 99, 'activeWindowId=99')
}

console.log('8. startAirTransferSharing returns real result (not fake true)')
{
  const hs = makeHarmonyShare(true)
  const air = makeAirTransfer(hs)
  startAirTransferSharing(air, async () => 42, { kind: 'text', text: 'x' }, undefined).then(result => {
    assert(result === false, 'failure -> false (not fake true)')
  })
}

console.log('9. startAirTransferSharing success returns true')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  startAirTransferSharing(air, async () => 42, { kind: 'text', text: 'x' }, undefined).then(result => {
    assert(result === true, 'success -> true')
  })
}

console.log('10. startAirTransferSharing no windowId + getWindowId null -> false')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  startAirTransferSharing(air, async () => null, { kind: 'text', text: 'x' }, undefined).then(result => {
    assert(result === false, 'no windowId -> false')
    assert(air.listening === false, 'not registered')
  })
}

console.log('11. startAirTransferSharing explicit windowId skips getWindowId')
{
  const hs = makeHarmonyShare(false)
  const air = makeAirTransfer(hs)
  let getWindowIdCalled = false
  startAirTransferSharing(air, async () => { getWindowIdCalled = true; return null }, { kind: 'text', text: 'x' }, 77).then(result => {
    assert(result === true, 'explicit windowId=77 success -> true')
    assert(getWindowIdCalled === false, 'getWindowId not called')
  })
}

import('node:timers').then(timers => {
  timers.setTimeout(() => {
    console.log('')
    console.log(`result: ${passed} passed, ${failed} failed`)
    if (failed > 0) process.exit(1)
  }, 100)
})

