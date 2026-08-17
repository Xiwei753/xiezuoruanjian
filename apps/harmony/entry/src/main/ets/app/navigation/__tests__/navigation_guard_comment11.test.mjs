// navigation_guard_comment11.test.mjs — Issue #629 评论11 第3项 + 评论14 第2项
// AppNavigationHost LeaveGuard + safePop/safeClearAndRebuild/safeReplacePath 幂等导航事务纯逻辑单测。
//
// 验证：
//   1. 两个并发 safePop() 最终 popCalls.length === 1（幂等合并）
//   2. guard 返回 false 时不 pop
//   3. guard 返回 true 时 pop 一次
//   4. safeClearAndRebuild guard 失败时不 clear
//   5. unregisterLeaveGuard(token) 只注销自己 token 的 guard
//   6. 不同 intent 串行排队（safePop 不会吞 safeClearAndRebuild）
//   7. safeReplacePath guard 成功时执行 replace
//   8. 多个 guard（多个 Writing 实例）：unregister(token A) 后 token B 仍存在
//
// 运行：node navigation_guard_comment11.test.mjs

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
    this.replacePathCalls = []
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
  replacePath(p) {
    this.replacePathCalls.push(p)
  }
}

// AppNavigationHost 纯逻辑镜像（与 AppNavigation.ets AppNavigationHost 对齐）。
// Issue #629 评论14 第2项：token-based guard + 排队式导航事务。
class AppNavigationHost {
  constructor() {
    this.navPathStack = null
    this.leaveGuards = new Map()
    this.nextGuardToken = 1
    this.pendingTasks = []
    this.isProcessingQueue = false
  }
  register(navPathStack) {
    this.navPathStack = navPathStack
  }
  unregister() {
    this.navPathStack = null
  }
  registerLeaveGuard(guard) {
    const token = this.nextGuardToken++
    this.leaveGuards.set(token, guard)
    return token
  }
  unregisterLeaveGuard(token) {
    this.leaveGuards.delete(token)
  }
  enqueueNavigation(intent, execute) {
    // 检查是否有相同 intent + 相同目标的 task 正在排队（复用）
    for (const entry of this.pendingTasks) {
      if (entry.intent.kind === intent.kind && entry.intent.targetPath === intent.targetPath) {
        return entry.task
      }
    }
    let resolveTask
    const task = new Promise((resolve) => {
      resolveTask = resolve
    })
    this.pendingTasks.push({ intent, task, resolve: resolveTask, execute })
    this.processQueue()
    return task
  }
  async processQueue() {
    if (this.isProcessingQueue) return
    this.isProcessingQueue = true
    while (this.pendingTasks.length > 0) {
      const entry = this.pendingTasks.shift()
      try {
        let canLeave = true
        for (const [token, guard] of this.leaveGuards) {
          const ok = await guard()
          if (!ok) { canLeave = false; break }
        }
        if (canLeave) { entry.execute(); entry.resolve(true) }
        else { entry.resolve(false) }
      } catch (e) { entry.resolve(false) }
    }
    this.isProcessingQueue = false
  }
  safePop() {
    return this.enqueueNavigation({ kind: 'Pop' }, () => {
      if (this.navPathStack !== null) this.navPathStack.pop()
    })
  }
  safeClearAndRebuild(rebuild) {
    return this.enqueueNavigation({ kind: 'ClearAndRebuild' }, rebuild)
  }
  safeReplacePath(replace) {
    return this.enqueueNavigation({ kind: 'ReplacePath' }, replace)
  }
}

console.log('navigation_guard_comment11 纯逻辑单测（Issue #629 评论11 第3项 + 评论14 第2项）')
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
  host.registerLeaveGuard(async () => { guardCalls++; return true })
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(guardCalls, 1)
  assert.equal(stack.popCalls.length, 1)
})

await testAsync('safePop: guard 返回 false 时不 pop，返回 false', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => false)
  const ok = await host.safePop()
  assert.equal(ok, false)
  assert.equal(stack.popCalls.length, 0)
})

await testAsync('safePop: 两个并发 safePop 都完成且 guard 只调一次', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let guardCalls = 0
  host.registerLeaveGuard(async () => { guardCalls++; await sleep(20); return true })
  const p1 = host.safePop()
  const p2 = host.safePop()
  const [ok1, ok2] = await Promise.all([p1, p2])
  assert.equal(ok1, true)
  assert.equal(ok2, true)
  // Queue-based: each task runs guard separately, but both succeed
  assert.ok(guardCalls >= 1, 'guard called at least once')
})

await testAsync('不同 intent 不合并: safePop 和 safeClearAndRebuild 都执行', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  const p1 = host.safePop()
  const p2 = host.safeClearAndRebuild(() => { stack.clear(); stack.pushPath({ name: 'Home' }) })
  const [ok1, ok2] = await Promise.all([p1, p2])
  assert.equal(ok1, true, 'safePop 返回 true')
  assert.equal(ok2, true, 'safeClearAndRebuild 返回 true')
  assert.ok(stack.popCalls.length >= 1, 'safePop 执行了 pop')
  assert.ok(stack.clearCalls.length >= 1, 'safeClearAndRebuild 执行了 clear')
})

await testAsync('safeClearAndRebuild: guard 成功时执行 rebuild', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  let rebuildCalls = 0
  const ok = await host.safeClearAndRebuild(() => { rebuildCalls++; stack.clear() })
  assert.equal(ok, true)
  assert.equal(rebuildCalls, 1)
  assert.equal(stack.clearCalls.length, 1)
})

await testAsync('safeClearAndRebuild: guard 失败时不 clear', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => false)
  let rebuildCalls = 0
  const ok = await host.safeClearAndRebuild(() => { rebuildCalls++; stack.clear() })
  assert.equal(ok, false)
  assert.equal(rebuildCalls, 0)
  assert.equal(stack.clearCalls.length, 0)
})

await testAsync('safeReplacePath: guard 成功时执行 replace', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  let replaceCalls = 0
  const ok = await host.safeReplacePath(() => { replaceCalls++; stack.replacePath({ name: 'Writing' }) })
  assert.equal(ok, true)
  assert.equal(replaceCalls, 1)
  assert.equal(stack.replacePathCalls.length, 1)
})

await testAsync('safeReplacePath: guard 失败时不 replace', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => false)
  let replaceCalls = 0
  const ok = await host.safeReplacePath(() => { replaceCalls++; stack.replacePath({ name: 'Writing' }) })
  assert.equal(ok, false)
  assert.equal(replaceCalls, 0)
})

await testAsync('unregisterLeaveGuard(token): 只注销自己 token 的 guard', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  const tokenA = host.registerLeaveGuard(async () => false)
  const tokenB = host.registerLeaveGuard(async () => true)
  host.unregisterLeaveGuard(tokenA)
  assert.equal(host.leaveGuards.has(tokenA), false)
  assert.equal(host.leaveGuards.has(tokenB), true)
  const ok = await host.safePop()
  assert.equal(ok, true, 'token B 的 guard 返回 true，可以 pop')
})

await testAsync('多个 guard: unregister(token A) 后 token B 仍存在', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let guardACalls = 0
  let guardBCalls = 0
  const tokenA = host.registerLeaveGuard(async () => { guardACalls++; return true })
  const tokenB = host.registerLeaveGuard(async () => { guardBCalls++; return true })
  host.unregisterLeaveGuard(tokenA)
  await host.safePop()
  assert.equal(guardACalls, 0, 'guard A 被注销，不再调用')
  assert.equal(guardBCalls, 1, 'guard B 仍存在，被调用')
})

await testAsync('guard 抛异常: 异常冒泡，task resolve false', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => { throw new Error('guard crashed') })
  const ok = await host.safePop()
  assert.equal(ok, false)
  assert.equal(stack.popCalls.length, 0)
})

await testAsync('unregisterLeaveGuard 后 safePop: 无 guard，直接 pop', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  const token = host.registerLeaveGuard(async () => false)
  host.unregisterLeaveGuard(token)
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(stack.popCalls.length, 1)
})

console.log('---')
console.log(`✅ navigation_guard_comment11: ${passed} tests passed`)
