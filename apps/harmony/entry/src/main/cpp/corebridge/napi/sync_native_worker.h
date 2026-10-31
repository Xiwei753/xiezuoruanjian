#ifndef SUJIAN_SYNC_NATIVE_WORKER_H
#define SUJIAN_SYNC_NATIVE_WORKER_H

// ── 同步长任务原生执行器（Issue #1011）──
//
// `napi_create_async_work` 的执行回调运行在 FFRT 工作线程上，可能持续数秒
// 至数分钟的 Git/网络调用会被 FFRT 长任务监测信号打断（EINTR），导致 TLS
// 读写以 `Interrupted system call` 失败。这里提供一个受控的原生工作线程：
// ArkTS 线程创建 Promise/deferred，工作线程只执行 Rust C ABI 并产生结果，
// 再通过 `napi_threadsafe_function` 投递回 ArkTS 线程 resolve。全量同步的
// perform / diagnostics / dry-run 共用本执行器，不再为每个入口复制线程系统。
//
// 生命周期：
// - TSFN 空闲时 unref，首个未完成任务到达时 ref，最后一条结果在
//   `SyncNativeCallJs` 中 resolve 完成后再次 unref。引用转换由同一个
//   pending 计数控制，避免工作线程阻止 ArkTS 事件循环退出，也避免事件循环
//   在结果投递前退出。
// - 环境销毁使用 `napi_add_async_cleanup_hook`：钩子关闭任务入口、解除事件
//   循环引用、丢弃排队任务并 abort TSFN 后立即返回，不阻塞 ArkTS 清理线程；
//   独立的清理线程等待工作线程真正退出后 join，再
//   `napi_remove_async_cleanup_hook`。工作线程不 detach，不会在模块卸载后
//   继续运行。
// - 工作线程在 `napi_call_threadsafe_function` 收到 `napi_closing` 后不再
//   使用该 TSFN，包括线程退出时不再调用 release；结果内存仍由丢弃路径释放。
// - 每个 napi_env 拥有独立的 state；TSFN 的 context 持有 state，直到 TSFN
//   销毁，保证 env 销毁期间排队回调仍能安全释放资源。
// - `writer_core_free_string` 只在正常回调或明确的丢弃路径调用一次，每条
//   结果恰好释放一次；丢弃路径可能位于工作线程或清理线程。

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <deque>
#include <exception>
#include <memory>
#include <mutex>
#include <new>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>

#include <hilog/log.h>
#include <napi/native_api.h>

#include "writer_core_bridge.h"

// 工作线程执行的 Rust C ABI；返回 Core 分配的 JSON 字符串（允许 nullptr）。
using SyncNativeExecuteFn = char* (*)();

struct SyncNativeTask {
    napi_deferred deferred = nullptr;
    SyncNativeExecuteFn execute = nullptr;
    const char* work_name = nullptr;
    char* result_json = nullptr;
    std::uint64_t task_id = 0;
    std::chrono::steady_clock::time_point queued_at;
    std::chrono::steady_clock::time_point rust_started_at;
    std::string diagnostic_operation_id;
};

struct SyncNativeTsfnContext;
struct SyncNativeCleanupContext;

struct SyncNativeWorkerState {
    explicit SyncNativeWorkerState(napi_env value) : env(value) {}

    napi_env env;
    std::mutex mutex;
    std::condition_variable cv;
    std::deque<SyncNativeTask*> queue;
    napi_threadsafe_function tsfn = nullptr;
    napi_async_cleanup_hook_handle cleanup_handle = nullptr;
    SyncNativeCleanupContext* cleanup_context = nullptr;
    std::thread worker;
    bool worker_started = false;
    bool stopping = false;
    // 任务已获准执行且尚未完成投递/丢弃。与 stopping 在同一把锁下切换，
    // 保证清理开始后不会再放行新的任务。
    bool executing = false;
    // TSFN 是否持有事件循环引用；ref/unref 只在创建它的 ArkTS 线程调用，
    // 字段本身由 mutex 保护。
    bool ref_held = false;
    // 已提交但尚未在 call_js 或丢弃路径结束的任务数（排队 + 执行 + 投递中）。
    std::size_t pending = 0;
};

// TSFN context：让 state 活到 TSFN 队列排空、finalize 回调执行。
struct SyncNativeTsfnContext {
    explicit SyncNativeTsfnContext(std::shared_ptr<SyncNativeWorkerState> value)
        : state(std::move(value)) {}

    std::shared_ptr<SyncNativeWorkerState> state;
};

// 异步清理钩子的参数由钩子侧负责释放。
struct SyncNativeCleanupContext {
    explicit SyncNativeCleanupContext(std::shared_ptr<SyncNativeWorkerState> value)
        : state(std::move(value)) {}

    std::shared_ptr<SyncNativeWorkerState> state;
};

static std::mutex g_sync_native_registry_mutex;
static std::unordered_map<napi_env, std::shared_ptr<SyncNativeWorkerState>> g_sync_native_states;
static std::atomic<std::uint64_t> g_sync_native_next_task_id{1};

static constexpr const char* kSyncNativeCreateFailedEnvelope =
    "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create sync native worker\"}";
static constexpr const char* kSyncNativeQueueFailedEnvelope =
    "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue sync native work\"}";
static constexpr const char* kSyncNativeNullResultEnvelope =
    "{\"success\":false,\"errorCode\":\"NULL_RESULT\",\"userMessage\":\"Sync operation returned no result\"}";

static const char* SyncNativeTaskWorkName(const SyncNativeTask* task) {
    return task != nullptr && task->work_name != nullptr ? task->work_name : "sync";
}

static const char* SyncNativeTaskOperationId(const SyncNativeTask* task) {
    if (task == nullptr || task->diagnostic_operation_id.empty()) {
        return "-";
    }
    return task->diagnostic_operation_id.c_str();
}

static long long SyncNativeElapsedMs(const std::chrono::steady_clock::time_point& start) {
    return static_cast<long long>(std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - start).count());
}

// 只提取信封里的稳定 operation id，不读取/记录任何结果正文。
// 仅接受字母、数字、'-'、'_'，长度上限 128；不符合时返回空串。
static std::string SyncNativeExtractOperationId(const char* json) {
    if (json == nullptr) {
        return {};
    }
    static constexpr const char kMarker[] = "\"diagnosticOperationId\":\"";
    const char* begin = std::strstr(json, kMarker);
    if (begin == nullptr) {
        return {};
    }
    begin += sizeof(kMarker) - 1;
    const char* end = std::strchr(begin, '"');
    if (end == nullptr || end == begin) {
        return {};
    }
    const std::size_t length = static_cast<std::size_t>(end - begin);
    if (length > 128) {
        return {};
    }
    for (const char* cursor = begin; cursor < end; ++cursor) {
        const char value = *cursor;
        const bool safe = (value >= 'a' && value <= 'z') || (value >= 'A' && value <= 'Z') ||
                          (value >= '0' && value <= '9') || value == '-' || value == '_';
        if (!safe) {
            return {};
        }
    }
    return std::string(begin, length);
}

static void SyncNativeResolveEnvelope(napi_env env, napi_deferred deferred, const char* json) {
    napi_value result = nullptr;
    if (napi_create_string_utf8(env, json, NAPI_AUTO_LENGTH, &result) == napi_ok) {
        napi_resolve_deferred(env, deferred, result);
    }
}

static void SyncNativeEraseState(napi_env env, SyncNativeWorkerState* state) {
    std::lock_guard<std::mutex> lock(g_sync_native_registry_mutex);
    auto found = g_sync_native_states.find(env);
    if (found != g_sync_native_states.end() && found->second.get() == state) {
        g_sync_native_states.erase(found);
    }
}

static std::shared_ptr<SyncNativeWorkerState> SyncNativeGetOrCreateState(napi_env env) {
    std::lock_guard<std::mutex> lock(g_sync_native_registry_mutex);
    auto found = g_sync_native_states.find(env);
    if (found != g_sync_native_states.end()) {
        return found->second;
    }

    try {
        auto state = std::make_shared<SyncNativeWorkerState>(env);
        g_sync_native_states.emplace(env, state);
        return state;
    } catch (const std::exception& error) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to allocate worker state: %{public}s", error.what());
        return nullptr;
    } catch (...) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to allocate worker state");
        return nullptr;
    }
}

// 任务结束但结果不会进入 call_js：释放结果并更新 pending。
// 该路径可能运行在工作线程或清理线程，不能调用 ref/unref。
static void SyncNativeTaskDiscard(const std::shared_ptr<SyncNativeWorkerState>& state, SyncNativeTask* task) {
    if (task == nullptr) {
        return;
    }
    if (task->result_json != nullptr) {
        writer_core_free_string(task->result_json);
        task->result_json = nullptr;
    }
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        if (state->pending > 0) {
            --state->pending;
        }
    }
    delete task;
}

// TSFN callback —— 运行在 ArkTS 所属线程，是正常结果 `result_json` 的唯一释放点。
// env 为 nullptr 表示 TSFN 正在 abort/环境已销毁：只释放资源，不调用 N-API。
static void SyncNativeCallJs(napi_env env, napi_value /*js_callback*/, void* context, void* data) {
    SyncNativeTask* task = static_cast<SyncNativeTask*>(data);
    if (task == nullptr) {
        return;
    }

    const char* work_name = SyncNativeTaskWorkName(task);
    const char* operation_id = SyncNativeTaskOperationId(task);

    SyncNativeTsfnContext* tsfn_context = static_cast<SyncNativeTsfnContext*>(context);
    if (tsfn_context == nullptr) {
        if (task->result_json != nullptr) {
            writer_core_free_string(task->result_json);
        }
        delete task;
        return;
    }

    std::shared_ptr<SyncNativeWorkerState> state = tsfn_context->state;
    bool arkts_received = false;
    if (env != nullptr && task->deferred != nullptr) {
        if (task->result_json != nullptr) {
            napi_value result = nullptr;
            if (napi_create_string_utf8(env, task->result_json, NAPI_AUTO_LENGTH, &result) == napi_ok) {
                arkts_received = napi_resolve_deferred(env, task->deferred, result) == napi_ok;
            }
        } else {
            // Rust 返回 nullptr：resolve 标准 ResultEnvelope，而不是 reject 成
            // 字符串后在 ArkTS catch 里丢成 NATIVE_ERROR。
            SyncNativeResolveEnvelope(env, task->deferred, kSyncNativeNullResultEnvelope);
            arkts_received = true;
        }
    }
    if (arkts_received) {
        OH_LOG_INFO(LOG_APP,
                    "SyncNative: arkts_received task=%{public}llu work=%{public}s elapsedMs=%{public}lld op=%{public}s",
                    static_cast<unsigned long long>(task->task_id), work_name,
                    SyncNativeElapsedMs(task->queued_at), operation_id);
    }

    if (task->result_json != nullptr) {
        writer_core_free_string(task->result_json);
        task->result_json = nullptr;
    }
    delete task;

    {
        std::lock_guard<std::mutex> lock(state->mutex);
        if (state->pending > 0) {
            --state->pending;
        }
        // 最后一条结果真正 resolve/释放后，才允许事件循环退出。与清理钩子
        // 共用 state->mutex，避免在 abort TSFN 之后再次 unref。
        if (state->pending == 0 && state->ref_held && env != nullptr &&
            !state->stopping && state->tsfn != nullptr) {
            if (napi_unref_threadsafe_function(env, state->tsfn) == napi_ok) {
                state->ref_held = false;
            } else {
                OH_LOG_WARN(LOG_APP, "SyncNative: failed to unref threadsafe function");
            }
        }
    }
}

// TSFN 是否已经进入不可再使用的关闭状态。HarmonyOS 与 Node-API 都以
// `napi_closing` 表示关闭；`napi_invalid_arg` 表示句柄已无效，也不能再触碰。
static bool SyncNativeTsfnIsClosed(napi_status status) {
    return status == napi_closing || status == napi_invalid_arg;
}

// 工作线程把结果投递回 ArkTS 线程；投递失败（TSFN 已关闭）时在这里释放结果。
// 成功后 task 的所有权转交 TSFN，工作线程不能再访问它。
// 返回值表示工作线程是否仍可继续使用该 TSFN；false 时不得再调用任何 TSFN API，
// 包括 `napi_release_threadsafe_function`。
static bool SyncNativeDeliver(const std::shared_ptr<SyncNativeWorkerState>& state,
                              napi_threadsafe_function tsfn,
                              SyncNativeTask* task) {
    if (tsfn == nullptr) {
        SyncNativeTaskDiscard(state, task);
        return false;
    }

    // 成功投递后 task 的所有权立即转交 TSFN，不能在调用后再读取其字段；
    // 因此先把稳定 ID 和耗时复制出来，再记录投递时间点。ArkTS 真正消费时由
    // SyncNativeCallJs 记录 arkts_received，两者可区分 TSFN 排队延迟。
    const std::uint64_t task_id = task->task_id;
    const char* work_name = SyncNativeTaskWorkName(task);
    const long long elapsed_ms = SyncNativeElapsedMs(task->queued_at);
    const std::string operation_id = SyncNativeExtractOperationId(task->result_json);

    napi_status status = napi_call_threadsafe_function(tsfn, task, napi_tsfn_nonblocking);
    if (status == napi_ok) {
        OH_LOG_INFO(LOG_APP,
                    "SyncNative: tsfn_delivered task=%{public}llu work=%{public}s elapsedMs=%{public}lld op=%{public}s",
                    static_cast<unsigned long long>(task_id), work_name, elapsed_ms,
                    operation_id.empty() ? "-" : operation_id.c_str());
        return true;
    }

    OH_LOG_WARN(LOG_APP, "SyncNative: failed to deliver %{public}s result: %{public}d",
                task->work_name != nullptr ? task->work_name : "sync", static_cast<int>(status));
    SyncNativeTaskDiscard(state, task);
    return !SyncNativeTsfnIsClosed(status);
}

// 唯一的长任务工作线程：串行执行队列中的 Rust 调用。
// pending 只在 call_js 或明确的丢弃路径完成后递减，因此排队、执行、投递三个
// 阶段的结束状态由同一生命周期计数发布，不存在“执行结束但结果尚未投递”的
// 假空闲窗口。
static void SyncNativeWorkerLoop(std::shared_ptr<SyncNativeWorkerState> state,
                                 napi_threadsafe_function tsfn) {
    bool tsfn_usable = tsfn != nullptr;
    std::unique_lock<std::mutex> lock(state->mutex);
    while (true) {
        state->cv.wait(lock, [&state] {
            return state->stopping || !state->queue.empty();
        });

        if (state->queue.empty()) {
            if (state->stopping) {
                break;
            }
            continue;
        }

        SyncNativeTask* task = state->queue.front();
        state->queue.pop_front();
        lock.unlock();

        // 与清理钩子共用一把锁完成“检查 stopping + 标记 executing”，
        // 清理一旦发布 stopping，就不会再放行新的任务；已获准的任务继续
        // 执行到返回，但结果不再投递。
        bool started = false;
        {
            std::lock_guard<std::mutex> state_lock(state->mutex);
            if (!state->stopping && tsfn_usable) {
                state->executing = true;
                started = true;
            }
        }

        bool stopping = !started;
        if (started) {
            task->rust_started_at = std::chrono::steady_clock::now();
            const long long wait_ms = static_cast<long long>(
                std::chrono::duration_cast<std::chrono::milliseconds>(
                    task->rust_started_at - task->queued_at).count());
            OH_LOG_INFO(LOG_APP,
                        "SyncNative: rust_start task=%{public}llu work=%{public}s waitMs=%{public}lld op=%{public}s",
                        static_cast<unsigned long long>(task->task_id),
                        SyncNativeTaskWorkName(task), wait_ms,
                        SyncNativeTaskOperationId(task));
            task->result_json = task->execute();
            task->diagnostic_operation_id = SyncNativeExtractOperationId(task->result_json);
            OH_LOG_INFO(LOG_APP,
                        "SyncNative: rust_return task=%{public}llu work=%{public}s elapsedMs=%{public}lld op=%{public}s",
                        static_cast<unsigned long long>(task->task_id),
                        SyncNativeTaskWorkName(task), SyncNativeElapsedMs(task->rust_started_at),
                        SyncNativeTaskOperationId(task));
            {
                std::lock_guard<std::mutex> state_lock(state->mutex);
                stopping = state->stopping;
            }
        }

        if (stopping || !tsfn_usable) {
            SyncNativeTaskDiscard(state, task);
        } else if (!SyncNativeDeliver(state, tsfn, task)) {
            // 已经收到关闭返回码：此后不得再对该句柄调用任何 TSFN API，
            // 包括退出时的 release；同时关闭任务入口并让工作线程尽快退出。
            tsfn_usable = false;
            {
                std::lock_guard<std::mutex> state_lock(state->mutex);
                state->stopping = true;
            }
            state->cv.notify_all();
        }

        if (started) {
            // 投递/丢弃都已完成，任务生命周期才结束。
            std::lock_guard<std::mutex> state_lock(state->mutex);
            state->executing = false;
        }

        lock.lock();
    }
    lock.unlock();

    // 只对仍由本线程持有且未观察到关闭的 TSFN 引用调用 release。模块引用
    // 由清理钩子 abort；收到 napi_closing 后句柄可能已经失效，不能再触碰。
    if (tsfn_usable) {
        napi_status status = napi_release_threadsafe_function(tsfn, napi_tsfn_release);
        if (status != napi_ok) {
            OH_LOG_WARN(LOG_APP, "SyncNative: failed to release threadsafe function: %{public}d",
                        static_cast<int>(status));
        }
    } else {
        OH_LOG_WARN(LOG_APP, "SyncNative: skip releasing a closed threadsafe function");
    }
}

// 异步清理的收尾线程：工作线程不可取消的 Rust 网络调用结束后才 join，
// 然后移除清理钩子，让环境销毁继续。这里不接触 napi_env，也不调用模块代码。
static void SyncNativeCompleteCleanup(SyncNativeCleanupContext* context,
                                      napi_async_cleanup_hook_handle handle) {
    std::shared_ptr<SyncNativeWorkerState> state = context->state;
    bool worker_started = false;
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        worker_started = state->worker_started;
    }
    if (worker_started && state->worker.joinable()) {
        state->worker.join();
    }
    SyncNativeEraseState(state->env, state.get());
    state->cleanup_handle = nullptr;
    state->cleanup_context = nullptr;
    delete context;
    context = nullptr;
    // 先释放本线程持有的 state 引用，再移除清理钩子；remove 返回后不再访问
    // 模块状态，避免环境随即卸载模块时清理线程仍在执行库内代码。
    state.reset();
    if (handle != nullptr) {
        napi_remove_async_cleanup_hook(handle);
    }
}

// 环境销毁钩子：只做关闭入口/解除事件循环引用/丢弃排队任务/abort TSFN，
// 然后交给独立收尾线程等待工作线程退出，避免在 ArkTS 清理线程上阻塞数分钟
// 的网络调用。
static void SyncNativeAsyncCleanup(napi_async_cleanup_hook_handle handle, void* arg) {
    SyncNativeCleanupContext* context = static_cast<SyncNativeCleanupContext*>(arg);
    if (context == nullptr) {
        if (handle != nullptr) {
            napi_remove_async_cleanup_hook(handle);
        }
        return;
    }

    std::shared_ptr<SyncNativeWorkerState> state = context->state;
    std::deque<SyncNativeTask*> pending;
    napi_threadsafe_function tsfn = nullptr;
    bool unref_tsfn = false;
    bool has_inflight = false;
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        state->stopping = true;
        pending.swap(state->queue);
        tsfn = state->tsfn;
        state->tsfn = nullptr;
        unref_tsfn = state->ref_held;
        state->ref_held = false;
        has_inflight = state->executing;
        state->cv.notify_all();
    }
    if (has_inflight) {
        OH_LOG_INFO(LOG_APP, "SyncNative: cleanup waits for an in-flight sync call");
    }

    // 清理钩子运行在 ArkTS 线程：先解除事件循环引用，再 abort 模块引用。
    // 工作线程仍持有自己的引用，因此 TSFN 不会在它退出前被释放。已经排队的
    // 投递会以 env == nullptr 回调并释放结果。
    if (tsfn != nullptr) {
        if (unref_tsfn && napi_unref_threadsafe_function(state->env, tsfn) != napi_ok) {
            OH_LOG_WARN(LOG_APP, "SyncNative: failed to unref threadsafe function during cleanup");
        }
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
    }

    for (SyncNativeTask* task : pending) {
        SyncNativeTaskDiscard(state, task);
    }

    // 收尾线程只负责 join + remove，不 detach 工作线程，也不越过模块生命周期。
    try {
        std::thread cleanup_thread(SyncNativeCompleteCleanup, context, handle);
        try {
            cleanup_thread.detach();
        } catch (...) {
            // detach 极少数情况下失败：就地等待，保证清理钩子最终被移除。
            if (cleanup_thread.joinable()) {
                cleanup_thread.join();
            }
        }
    } catch (const std::exception& error) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start cleanup thread: %{public}s", error.what());
        SyncNativeCompleteCleanup(context, handle);
    } catch (...) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start cleanup thread");
        SyncNativeCompleteCleanup(context, handle);
    }
}

// TSFN 销毁时释放 context，确保排队回调结束后 state 才可能析构。
static void SyncNativeTsfnFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
    delete static_cast<SyncNativeTsfnContext*>(data);
}

// 所有创建步骤成功后才把 state 发布到注册表；失败时撤销钩子并移除 state。
static void SyncNativeEnsureRollback(const std::shared_ptr<SyncNativeWorkerState>& state) {
    if (state->cleanup_handle != nullptr) {
        napi_remove_async_cleanup_hook(state->cleanup_handle);
        state->cleanup_handle = nullptr;
    }
    delete state->cleanup_context;
    state->cleanup_context = nullptr;
    SyncNativeEraseState(state->env, state.get());
}

// 在 ArkTS 线程准备 TSFN / 工作线程。只在首次提交时创建，后续复用。
static bool SyncNativeWorkerEnsure(const std::shared_ptr<SyncNativeWorkerState>& state) {
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        if (state->stopping) {
            return false;
        }
        if (state->tsfn != nullptr && state->worker_started) {
            return true;
        }
    }

    napi_env env = state->env;

    // 先注册异步清理钩子；后续任一步失败都由 rollback 撤销。
    if (state->cleanup_handle == nullptr) {
        auto* cleanup_context = new (std::nothrow) SyncNativeCleanupContext(state);
        if (cleanup_context == nullptr) {
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to allocate cleanup context");
            return false;
        }

        napi_status status = napi_add_async_cleanup_hook(
            env, SyncNativeAsyncCleanup, cleanup_context, &state->cleanup_handle);
        if (status != napi_ok) {
            delete cleanup_context;
            state->cleanup_handle = nullptr;
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to install async cleanup hook: %{public}d",
                         static_cast<int>(status));
            SyncNativeEraseState(env, state.get());
            return false;
        }
        state->cleanup_context = cleanup_context;
    }

    napi_value resource_name = nullptr;
    if (napi_create_string_utf8(env, "SujianSyncNativeWorker", NAPI_AUTO_LENGTH,
                                &resource_name) != napi_ok) {
        SyncNativeEnsureRollback(state);
        return false;
    }

    auto* tsfn_context = new (std::nothrow) SyncNativeTsfnContext(state);
    if (tsfn_context == nullptr) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to allocate threadsafe context");
        SyncNativeEnsureRollback(state);
        return false;
    }

    napi_threadsafe_function tsfn = nullptr;
    napi_status status = napi_create_threadsafe_function(
        env, nullptr, nullptr, resource_name,
        0,     // max_queue_size: 0 = 不限制，避免大文件同步时投递排队失败
        1,     // initial_thread_count: 工作线程
        tsfn_context, SyncNativeTsfnFinalize, tsfn_context,
        SyncNativeCallJs, &tsfn);
    if (status != napi_ok) {
        delete tsfn_context;
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to create threadsafe function: %{public}d",
                     static_cast<int>(status));
        SyncNativeEnsureRollback(state);
        return false;
    }

    // 模块自身也持有一个引用，保证清理时可以先 abort 再等工作线程退出。
    if (napi_acquire_threadsafe_function(tsfn) != napi_ok) {
        // 此时只有 initial_thread_count 的引用。
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to acquire threadsafe function");
        SyncNativeEnsureRollback(state);
        return false;
    }

    // 刚创建时 TSFN 默认引用事件循环；空闲阶段必须 unref。
    if (napi_unref_threadsafe_function(env, tsfn) != napi_ok) {
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to unref threadsafe function");
        SyncNativeEnsureRollback(state);
        return false;
    }

    {
        std::lock_guard<std::mutex> lock(state->mutex);
        state->tsfn = tsfn;
        state->ref_held = false;
    }

    try {
        state->worker = std::thread(SyncNativeWorkerLoop, state, tsfn);
        {
            std::lock_guard<std::mutex> lock(state->mutex);
            state->worker_started = true;
        }
    } catch (const std::exception& error) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start worker thread: %{public}s", error.what());
        {
            std::lock_guard<std::mutex> lock(state->mutex);
            state->tsfn = nullptr;
        }
        // 释放模块引用和 initial_thread_count 引用。
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        SyncNativeEnsureRollback(state);
        return false;
    } catch (...) {
        OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start worker thread");
        {
            std::lock_guard<std::mutex> lock(state->mutex);
            state->tsfn = nullptr;
        }
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        napi_release_threadsafe_function(tsfn, napi_tsfn_abort);
        SyncNativeEnsureRollback(state);
        return false;
    }

    return true;
}

static bool SyncNativeWorkerEnqueue(const std::shared_ptr<SyncNativeWorkerState>& state,
                                    SyncNativeTask* task) {
    std::lock_guard<std::mutex> lock(state->mutex);
    if (state->stopping || state->tsfn == nullptr) {
        return false;
    }

    // 0 → 1：第一个未完成任务出现时重新引用事件循环。ref/unref 只在 ArkTS
    // 线程调用；这里由 SyncNativeWorkerSubmit 保证。
    if (state->pending == 0 && !state->ref_held) {
        if (napi_ref_threadsafe_function(state->env, state->tsfn) != napi_ok) {
            return false;
        }
        state->ref_held = true;
    }

    ++state->pending;
    try {
        state->queue.push_back(task);
    } catch (...) {
        --state->pending;
        if (state->pending == 0 && state->ref_held) {
            if (napi_unref_threadsafe_function(state->env, state->tsfn) == napi_ok) {
                state->ref_held = false;
            }
        }
        return false;
    }
    state->cv.notify_one();
    return true;
}

// 创建 Promise 并把长任务交给唯一的工作线程；返回的 Promise 语义与旧的
// napi_create_async_work 入口一致（成功/业务失败都 resolve ResultEnvelope）。
static napi_value SyncNativeWorkerSubmit(napi_env env, const char* work_name, SyncNativeExecuteFn execute) {
    napi_deferred deferred = nullptr;
    napi_value promise = nullptr;
    if (napi_create_promise(env, &deferred, &promise) != napi_ok) {
        return nullptr;
    }

    std::shared_ptr<SyncNativeWorkerState> state = SyncNativeGetOrCreateState(env);
    if (state == nullptr || !SyncNativeWorkerEnsure(state)) {
        SyncNativeResolveEnvelope(env, deferred, kSyncNativeCreateFailedEnvelope);
        return promise;
    }

    SyncNativeTask* task = new (std::nothrow) SyncNativeTask();
    if (task == nullptr) {
        SyncNativeResolveEnvelope(env, deferred, kSyncNativeCreateFailedEnvelope);
        return promise;
    }
    task->deferred = deferred;
    task->execute = execute;
    task->work_name = work_name;
    task->task_id = g_sync_native_next_task_id.fetch_add(1, std::memory_order_relaxed);
    task->queued_at = std::chrono::steady_clock::now();

    if (!SyncNativeWorkerEnqueue(state, task)) {
        delete task;
        SyncNativeResolveEnvelope(env, deferred, kSyncNativeQueueFailedEnvelope);
    } else {
        OH_LOG_INFO(LOG_APP,
                    "SyncNative: queued task=%{public}llu work=%{public}s elapsedMs=0 op=%{public}s",
                    static_cast<unsigned long long>(task->task_id),
                    SyncNativeTaskWorkName(task), SyncNativeTaskOperationId(task));
    }
    return promise;
}

#endif  // SUJIAN_SYNC_NATIVE_WORKER_H
