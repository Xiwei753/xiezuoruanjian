// navigation_guard_comment11.test.mjs — Issue #629 评论11 第3项 + 评论14 第2项 + 评论15 第6项
// AppNavigationHost LeaveGuard + safePop/safeClearAndRebuild/safeReplacePath 幂等导航事务纯逻辑单测。
//
// 验证（评论15 第6项 恢复强断言）：
//   1. 两个并发 safePop() → guardCalls===1 且 popCalls.length===1
//   2. guard 返回 false 时不 pop
//   3. safeClearAndRebuild guard 失败时不 clear
//   4. activeGuardLease：旧 Writing token 注销后新 guard 仍在
//   5. 不同 intent 串行排队（safePop 不会吞 safeClearAndRebuild）
//   6. safeReplacePath：相同 targetKey 复用；不同 targetKey 串行执行
//   7. Pop 正在执行时 ClearAndRebuild 入队，两个 intent 都执行且顺序稳定
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

// MockNavPathStack：记录 pop/clear/replacePath 调用。
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
// Issue #629 评论14 第2项 + 评论15 第6项：
// - activeGuardLease: 只持有当前 active guard，旧实例迟到的 disappear 不会删新 guard
// - NavigationIntent dedupeKey: Pop='pop', ReplacePath='replace:<targetKey>', ClearAndRebuild 由调用方提供
// - activeTask: 正在执行的任务仍在"可去重集合"里（shift 后不消失）
// - 不同 intent 严格串行执行，不同 dedupeKey 不合并
class AppNavigationHost {
  constructor() {
    this.navPathStack = null
    this.activeGuardLease = null
    this.pendingTasks = []
    this.activeTask = null
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
    // Issue #629 评论14 第2项：新 Writing 注册时替换 active lease
    this.activeGuardLease = { token, guard }
    return token
  }
  nextGuardToken = 1
  unregisterLeaveGuard(token) {
    // 只有 token 等于当前 active lease 才清掉
    if (this.activeGuardLease !== null && this.activeGuardLease.token === token) {
      this.activeGuardLease = null
    }
  }
  enqueueNavigation(intent, execute) {
    // Issue #629 评论14 第2项 + 评论15 第6项：去重同时检查 pendingTasks + activeTask
    for (const entry of this.pendingTasks) {
      if (entry.intent.dedupeKey === intent.dedupeKey) {
        return entry.task
      }
    }
    if (this.activeTask !== null && this.activeTask.intent.dedupeKey === intent.dedupeKey) {
      return this.activeTask.task
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
      // shift 后立即设置 activeTask
      this.activeTask = { intent: entry.intent, task: entry.task }
      try {
        // Issue #629 评论14 第2项：只执行当前 active guard（不是遍历所有历史 guard）
        let canLeave = true
        const lease = this.activeGuardLease
        if (lease !== null) {
          canLeave = await lease.guard()
        }
        if (canLeave) {
          entry.execute()
          entry.resolve(true)
        } else {
          entry.resolve(false)
        }
      } catch (_e) {
        entry.resolve(false)
      } finally {
        if (this.activeTask !== null && this.activeTask.task === entry.task) {
          this.activeTask = null
        }
      }
    }
    this.isProcessingQueue = false
  }
  safePop() {
    const intent = { kind: 'Pop', dedupeKey: 'pop' }
    return this.enqueueNavigation(intent, () => {
      if (this.navPathStack !== null) this.navPathStack.pop()
    })
  }
  safeClearAndRebuild(dedupeKey, rebuild) {
    const intent = { kind: 'ClearAndRebuild', dedupeKey }
    return this.enqueueNavigation(intent, rebuild)
  }
  safeReplacePath(targetKey, replace) {
    const intent = { kind: 'ReplacePath', dedupeKey: `replace:${targetKey}` }
    return this.enqueueNavigation(intent, replace)
  }
}

console.log('navigation_guard_comment11 纯逻辑单测（Issue #629 评论15 第6项恢复强断言）')
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

await testAsync('safePop: 两个并发 safePop → guardCalls===1 且 popCalls.length===1（强断言）', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let guardCalls = 0
  host.registerLeaveGuard(async () => { guardCalls++; await sleep(20); return true })
  const p1 = host.safePop()
  const p2 = host.safePop()
  // Issue #629 评论17 第3项：两个并发 safePop 必须复用同一个 Promise（不是值相同）。
  assert.equal(p1, p2, '两个并发 safePop 必须复用同一个 Promise')
  const [ok1, ok2] = await Promise.all([p1, p2])
  // Issue #629 评论15 第6项：两个 safePop 同 dedupeKey 复用同一 task
  assert.equal(ok1, ok2, '两个 safePop 应返回相同 result')
  assert.equal(ok1, true)
  assert.equal(guardCalls, 1, 'guard 只调一次（dedup 合并）')
  assert.equal(stack.popCalls.length, 1, 'pop 只执行一次（dedup 合并）')
})

await testAsync('activeGuardLease: 旧 Writing token 注销后新 guard 仍在', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  const tokenA = host.registerLeaveGuard(async () => false)
  const tokenB = host.registerLeaveGuard(async () => true)
  host.unregisterLeaveGuard(tokenA)
  assert.equal(host.activeGuardLease.token, tokenB, 'token B 仍是 active lease')
  const ok = await host.safePop()
  assert.equal(ok, true, 'token B 的 guard 返回 true，可以 pop')
})

await testAsync('不同 intent 不合并: safePop 和 safeClearAndRebuild 都执行且按顺序', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  const order = []
  const p1 = host.safePop().then(ok => { order.push('pop'); return ok })
  const p2 = host.safeClearAndRebuild('k', () => { order.push('clear'); stack.clear(); stack.pushPath({ name: 'Home' }) }).then(ok => { order.push('clearDone'); return ok })
  const [ok1, ok2] = await Promise.all([p1, p2])
  assert.equal(ok1, true, 'safePop 返回 true')
  assert.equal(ok2, true, 'safeClearAndRebuild 返回 true')
  assert.equal(stack.popCalls.length, 1, 'pop 执行一次')
  assert.equal(stack.clearCalls.length, 1, 'clear 执行一次')
  // 顺序：pop 先入队先执行，clear 后入队后执行
  assert.ok(order.indexOf('pop') < order.indexOf('clear'), 'pop 在 clear 之前执行')
})

await testAsync('safeClearAndRebuild: guard 失败时不 clear', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => false)
  let rebuildCalls = 0
  const ok = await host.safeClearAndRebuild('k', () => { rebuildCalls++; stack.clear() })
  assert.equal(ok, false)
  assert.equal(rebuildCalls, 0)
  assert.equal(stack.clearCalls.length, 0)
})

await testAsync('safeReplacePath: 相同 targetKey 复用同一 task', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  const p1 = host.safeReplacePath('ch1', () => { stack.replacePath({ name: 'Writing' }) })
  const p2 = host.safeReplacePath('ch1', () => { stack.replacePath({ name: 'Writing' }) })
  assert.equal(p1, p2, '相同 targetKey 复用 Promise')
  const [ok1] = await Promise.all([p1, p2])
  assert.equal(ok1, true)
  assert.equal(stack.replacePathCalls.length, 1, 'replace 只执行一次')
})

await testAsync('safeReplacePath: 不同 targetKey 串行执行两次，不互吞', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  host.registerLeaveGuard(async () => true)
  const p1 = host.safeReplacePath('ch1', () => { stack.replacePath({ name: 'Writing', param: { chapterId: 'ch1' } }) })
  const p2 = host.safeReplacePath('ch2', () => { stack.replacePath({ name: 'Writing', param: { chapterId: 'ch2' } }) })
  const [ok1, ok2] = await Promise.all([p1, p2])
  assert.equal(ok1, true)
  assert.equal(ok2, true)
  assert.equal(stack.replacePathCalls.length, 2, '两个不同 chapter 各执行一次 replace')
})

await testAsync('Pop 正在执行时 ClearAndRebuild 入队 → 两个都按顺序执行', async () => {
  const host = new AppNavigationHost()
  const stack = new MockNavPathStack()
  host.register(stack)
  let guardResolve = null
  const guardPromise = new Promise((resolve) => { guardResolve = resolve })
  host.registerLeaveGuard(async () => guardPromise)
  const order = []
  // 第一个 safePop 开始（guard 等待中）
  const p1 = host.safePop().then(ok => { order.push('pop'); return ok })
  await sleep(10)
  // 第二个 safeClearAndRebuild 入队（不同 dedupeKey，不会被吞）
  const p2 = host.safeClearAndRebuild('k', () => { order.push('clear'); stack.clear() }).then(ok => { order.push('clearDone'); return ok })
  await sleep(10)
  // 放行 guard
  guardResolve(true)
  await sleep(50)
  // 等待两个都完成
  await Promise.all([p1, p2])
  assert.equal(stack.popCalls.length, 1)
  assert.equal(stack.clearCalls.length, 1)
  // 顺序：pop 先执行，clear 后执行
  assert.ok(order.indexOf('pop') < order.indexOf('clear'), 'pop 在 clear 之前')
})

await testAsync('guard 抛异常 → task resolve false', async () => {
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
  assert.equal(host.activeGuardLease, null, 'activeGuardLease 已清空')
  const ok = await host.safePop()
  assert.equal(ok, true)
  assert.equal(stack.popCalls.length, 1)
})

console.log('---')
console.log(`✅ navigation_guard_comment11: ${passed} tests passed`)
