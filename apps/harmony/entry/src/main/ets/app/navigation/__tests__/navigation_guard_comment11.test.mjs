// navigation_guard_comment11.test.mjs — Issue #629 评论11 第3项
// AppNavigationHost LeaveGuard + safePop/safeClearAndRebuild 幂等导航事务纯逻辑单测。
//
// 验证：
//   1. 两个并发 safePop() 最终 popCalls.length === 1（幂等合并）
//   2. guard 返回 false 时不 pop
//   3. guard 返回 true 时 pop 一次
//   4. safeClearAndRebuild guard 失败时不 clear
//   5. unregisterLeaveGuard 只注销自己 owner 的 guard（别的 owner 注销不动）
//   6. navigationTask 串行：第一个 task 未完成时第二个 safePop 复用同一 task
//
// 运行：node navigation_guard_comment11.test.mjs
//
// 注意：.ets 依赖 ArkUI 无法用 Node 直接测，本测试验证提取的纯逻辑
// （AppNavigationHost 的 leaveGuard + navigationTask 编排）。
// 生产代码 AppNavigation.ets 用相同模式，需 HarmonyOS SDK 才能端到端编译——
// 本地无 SDK，此为已知阻塞。

import { strict as assert } from 'node:assert'

let passed = 0
const testAsync = async (name, fn) => {
  await fn()
  passed++
  console.log(`  [PASS] ${name}`)
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms))

// MockNavPathStack：记录 pop/clear/pushPath 调用。
class MockNavPathStack {
  constructor() {
    this.popCalls = []
    this.clearCalls = []
    this.pushPathCalls = []
  }
  pop() {
    this.popCalls.push(Date.now())
  }
  clear() {
    this.clearCalls.push(Date.now())
  }
  pushPath(p) {
    this.pushPathCalls.push(p)
  }
}

// AppNavigationHost 纯逻辑镜像（与 AppNavigation.ets AppNavigationHost 对齐）。
// 不依赖 ArkUI NavPathStack 类型，用 mock。
// 关键：runGuardedNavigation/safePop/safeClearAndRebuild 不是 async 函数，
// 直接返回 Promise 引用（async 函数会包一层新 Promise，破坏引用相等性）。
class AppNavigationHost {
  constructor() {
    this.navPathStack = null
    this.leaveGuard = null
    this.leaveGuardOwner = null
    this.navigationTask = null
  }
  register(navPathStack) {
    this.navPathStack = navPathStack
  }
  unregister() {
    this.navPathStack = null
  }
  registerLeaveGuard(owner, guard) {
    this.leaveGuard = guard
    this.leaveGuardOwner = owner
  }
  unregisterLeaveGuard(owner) {
    if (this.leaveGuardOwner === owner) {
      this.leaveGuard = null
      this.leaveGuardOwner = null
    }
  }
  // 与 AppNavigation.ets runGuardedNavigation 对齐。
  // 不是 async 函数：直接返回 Promise 引用，保证 navigationTask 复用时返回同一 Promise。
  // 用 .then() 清 navigationTask（避免 const task TDZ）。
  runGuardedNavigation(mutation) {
    if (this.navigationTask !== null) {
      return this.navigationTask
    }
    const task = (async () => {
      const guard = this.leaveGuard
      if (guard !== null) {
        const ok = await guard()
        if (!ok) {
          return false
        }
      }
      mutation()
      return true
    })()
    this.navigationTask = task
    task.then(
      () => { if (this.navigationTask === task) this.navigationTask = null },
      () => { if (this.navigationTask === task) this.navigationTask = null }
    )
    return task
  }
  safePop() {
    return this.runGuardedNavigation(() => {
      if (this.navPathStack !== null) {
        this.navPathStack.pop()
      }
    })
  }
  safeClearAndRebuild(rebuild) {
    return this.runGuardedNavigation(rebuild)
  }
}

console.log('navigation_guard_comment11 纯逻辑单测（Issue #629 评论11 第3项）')
console.log('---')

await testAsync('safePop: 无 guard 时 pop 一次，返回 true', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('safePop: guard 返回 true 时 pop 一次，返回 true', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let guardCalls = 0
  host.registerLeaveGuard('writing-screen', async () => { guardCalls++; return true })
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(guardCalls, 1)
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('safePop: guard 返回 false 时不 pop，返回 false', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => false)
  const ok = await host.safePop()
  assert.equal(ok, false)
  assert.equal(stack.popCalls.length, 0)
})

await testAsync('safePop 幂等: 两个并发 safePop 最终 popCalls.length === 1（navigationTask 复用同一 task）', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => { await sleep(20); return true })
  const [ok1, ok2] = await Promise.all([host.safePop(), host.safePop()])
  assert.equal(ok1, true)
  assert.equal(ok2, true)
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('safePop 串行: 第一个 task 未完成时第二个 safePop 复用同一 task（返回同一 Promise）', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => { await sleep(30); return true })
  const p1 = host.safePop()
  const p2 = host.safePop()
  assert.equal(p1, p2)
  await p1
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('safePop 串行: 第一个 task 完成后 navigationTask 清空，第二次 safePop 启动新 task', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => true)
  await host.safePop()
  await Promise.resolve()
  assert.equal(host.navigationTask, null, 'task 完成后 navigationTask 清空')
  await host.safePop()
  assert.equal(stack.popCalls.length, 2, '第二次 safePop 是新 task，pop 第二次')
})

await testAsync('safeClearAndRebuild: guard 成功时执行 rebuild（含 clear + pushPath）', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => true)
  let rebuildCalls = 0
  const ok = await host.safeClearAndRebuild(() => {
    rebuildCalls++
    stack.clear()
    stack.pushPath({ name: 'Home' })
  })
  assert.equal(ok, true)
  assert.equal(rebuildCalls, 1)
  assert.equal(stack.clearCalls.length, 1)
  assert.equal(stack.pushPathCalls.length, 1)
  assert.equal(stack.pushPathCalls[0].name, 'Home')
})

await testAsync('safeClearAndRebuild: guard 失败时不 clear，返回 false', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => false)
  let rebuildCalls = 0
  const ok = await host.safeClearAndRebuild(() => {
    rebuildCalls++
    stack.clear()
  })
  assert.equal(ok, false)
  assert.equal(rebuildCalls, 0)
  assert.equal(stack.clearCalls.length, 0)
})

await testAsync('safeClearAndRebuild 幂等: 两个并发合并成一次', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => { await sleep(20); return true })
  let rebuildCalls = 0
  const rebuild = () => { rebuildCalls++; stack.clear() }
  const [ok1, ok2] = await Promise.all([
    host.safeClearAndRebuild(rebuild),
    host.safeClearAndRebuild(rebuild),
  ])
  assert.equal(ok1, true)
  assert.equal(ok2, true)
  assert.equal(rebuildCalls, 1, 'rebuild 只执行一次')
  assert.equal(stack.clearCalls.length, 1)
})

await testAsync('unregisterLeaveGuard: 只注销自己 owner 的 guard', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => false)
  host.unregisterLeaveGuard('other-screen')
  assert.notEqual(host.leaveGuard, null)
  assert.equal(host.leaveGuardOwner, 'writing-screen')
  host.unregisterLeaveGuard('writing-screen')
  assert.equal(host.leaveGuard, null)
  assert.equal(host.leaveGuardOwner, null)
})

await testAsync('unregisterLeaveGuard 后 safePop: 无 guard，直接 pop', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => false)
  host.unregisterLeaveGuard('writing-screen')
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('guard 抛异常: runGuardedNavigation 不 swallow，异常冒泡，navigationTask 清空', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard('writing-screen', async () => { throw new Error('guard crashed') })
  let threw = false
  try {
    await host.safePop()
  } catch (e) {
    threw = true
  }
  assert.equal(threw, true)
  assert.equal(stack.popCalls.length, 0)
  await Promise.resolve()
  assert.equal(host.navigationTask, null)
})

await testAsync('mutation 抛异常: navigationTask 清空，允许后续新导航', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let threw = false
  try {
    await host.safeClearAndRebuild(() => { throw new Error('rebuild crashed') })
  } catch (e) {
    threw = true
  }
  assert.equal(threw, true)
  await Promise.resolve()
  assert.equal(host.navigationTask, null)
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(stack.popCalls.length, 1)
})

console.log('---')
console.log(`✅ navigation_guard_comment11: ${passed} tests passed`)
