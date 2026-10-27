#ifndef SUJIAN_SYNC_NATIVE_WORKER_H
#define SUJIAN_SYNC_NATIVE_WORKER_H

// ── 同步长任务原生执行器（Issue #1011）──
//
// `napi_create_async_work` 的执行回调运行在 FFRT 工作线程上，可能持续数秒
// 至数分钟的 Git/网络调用会被 FFRT 长任务监测信号打断（EINTR），导致 TLS
// 读写以 `Interrupted system call` 失败。这里提供一个模块级、唯一的受控
// pthread 工作线程：ArkTS 线程创建 Promise/deferred，工作线程只执行 Rust
// C ABI 并产生结果，再通过 `napi_threadsafe_function` 投递回 ArkTS 线程
// resolve。全量同步的 perform / diagnostics / dry-run 共用本执行器，不再为
// 每个入口复制线程系统。
//
// 生命周期：
// - 首次提交时在 ArkTS 线程创建 TSFN 并启动工作线程；之后复用。
// - 环境销毁时停止队列、abort TSFN、释放未执行任务；若 Rust 调用仍在执行，
//   因不可取消而 detach，由工作线程持有 state 自行释放。
// - `writer_core_free_string` 只在 ArkTS 线程的 TSFN 回调里调用一次。

#include <condition_variable>
#include <cstring>
#include <deque>
#include <exception>
#include <memory>
#include <mutex>
#include <thread>

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
};

struct SyncNativeWorkerState {
    std::mutex mutex;
    std::condition_variable cv;
    std::deque<SyncNativeTask*> queue;
    napi_threadsafe_function tsfn = nullptr;
    bool stopping = false;
    bool executing = false;
};

static std::shared_ptr<SyncNativeWorkerState> g_sync_native_state;
static std::thread g_sync_native_thread;
static bool g_sync_native_thread_started = false;
static napi_env g_sync_native_env = nullptr;
static bool g_sync_native_cleanup_installed = false;

static constexpr const char* kSyncNativeCreateFailedEnvelope =
    "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create sync native worker\"}";
static constexpr const char* kSyncNativeQueueFailedEnvelope =
    "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue sync native work\"}";
static constexpr const char* kSyncNativeNullResultEnvelope =
    "{\"success\":false,\"errorCode\":\"NULL_RESULT\",\"userMessage\":\"Sync operation returned no result\"}";

static void SyncNativeResolveEnvelope(napi_env env, napi_deferred deferred, const char* json) {
    napi_value result = nullptr;
    if (napi_create_string_utf8(env, json, NAPI_AUTO_LENGTH, &result) == napi_ok) {
        napi_resolve_deferred(env, deferred, result);
    }
}

// TSFN callback —— 运行在 ArkTS 所属线程，是 result_json 的唯一释放点。
// env 为 nullptr 表示 thread-safe function 正在 abort/环境已销毁：只释放资源。
static void SyncNativeCallJs(napi_env env, napi_value /*js_callback*/, void* /*context*/, void* data) {
    SyncNativeTask* task = static_cast<SyncNativeTask*>(data);
    if (task == nullptr) {
        return;
    }

    if (env != nullptr && task->deferred != nullptr) {
        if (task->result_json != nullptr) {
            napi_value result = nullptr;
            if (napi_create_string_utf8(env, task->result_json, NAPI_AUTO_LENGTH, &result) == napi_ok) {
                napi_resolve_deferred(env, task->deferred, result);
            }
        } else {
            // Rust 返回 nullptr：resolve 标准 ResultEnvelope，而不是 reject 成
            // 字符串后在 ArkTS catch 里丢成 NATIVE_ERROR。
            SyncNativeResolveEnvelope(env, task->deferred, kSyncNativeNullResultEnvelope);
        }
    }

    if (task->result_json != nullptr) {
        writer_core_free_string(task->result_json);
        task->result_json = nullptr;
    }
    delete task;
}

// 工作线程把结果投递回 ArkTS 线程；投递失败（TSFN 已关闭）时在这里释放结果。
static void SyncNativeDeliver(const std::shared_ptr<SyncNativeWorkerState>& state, SyncNativeTask* task) {
    napi_threadsafe_function tsfn = state->tsfn;
    if (tsfn == nullptr) {
        if (task->result_json != nullptr) {
            writer_core_free_string(task->result_json);
        }
        delete task;
        return;
    }

    napi_status status = napi_call_threadsafe_function(tsfn, task, napi_tsfn_nonblocking);
    if (status != napi_ok) {
        if (task->result_json != nullptr) {
            writer_core_free_string(task->result_json);
            task->result_json = nullptr;
        }
        OH_LOG_WARN(LOG_APP, "SyncNative: failed to deliver %{public}s result: %{public}d",
                    task->work_name != nullptr ? task->work_name : "sync", static_cast<int>(status));
        delete task;
    }
}

// 唯一的同步长任务工作线程：串行执行队列中的 Rust 调用。
static void SyncNativeWorkerLoop(std::shared_ptr<SyncNativeWorkerState> state) {
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
        state->executing = true;
        lock.unlock();

        task->result_json = task->execute();

        lock.lock();
        state->executing = false;
        state->cv.notify_all();
        lock.unlock();

        SyncNativeDeliver(state, task);

        lock.lock();
    }

    napi_threadsafe_function tsfn = state->tsfn;
    lock.unlock();
    if (tsfn != nullptr) {
        // 释放工作线程自己持有的 TSFN 引用；模块持有的引用由 cleanup hook 释放。
        napi_release_threadsafe_function(tsfn, napi_tsfn_release);
    }
}

// 环境销毁：停止接收新任务，abort TSFN 并回收未执行任务。
static void SyncNativeCleanupHook(void* /*arg*/) {
    std::shared_ptr<SyncNativeWorkerState> state = g_sync_native_state;
    if (state == nullptr) {
        g_sync_native_env = nullptr;
        g_sync_native_cleanup_installed = false;
        return;
    }

    std::deque<SyncNativeTask*> pending;
    bool executing = false;
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        state->stopping = true;
        pending.swap(state->queue);
        executing = state->executing;
        state->cv.notify_all();
    }

    // 丢弃模块持有的 TSFN 引用，并让已排队的投递以 env == nullptr 回调，
    // 从而在 SyncNativeCallJs 里释放 result_json 和 task。
    if (state->tsfn != nullptr) {
        napi_release_threadsafe_function(state->tsfn, napi_tsfn_abort);
    }

    for (SyncNativeTask* task : pending) {
        if (task->result_json != nullptr) {
            writer_core_free_string(task->result_json);
        }
        delete task;
    }

    if (g_sync_native_thread.joinable()) {
        if (executing) {
            // Rust 网络调用不可取消；阻塞模块卸载直到它结束会让应用退出卡住。
            // 工作线程持有 state 的 shared_ptr，完成投递/释放后自行退出。
            g_sync_native_thread.detach();
        } else {
            g_sync_native_thread.join();
        }
    }

    g_sync_native_thread_started = false;
    g_sync_native_state.reset();
    g_sync_native_env = nullptr;
    g_sync_native_cleanup_installed = false;
}

// TSFN 创建成功但工作线程启动失败：释放模块引用和尚未交给工作线程的
// initial_thread_count 引用，避免 TSFN 泄漏。仅在尚未启动线程的路径调用。
static void SyncNativeDiscardTsfn() {
    if (g_sync_native_state == nullptr || g_sync_native_state->tsfn == nullptr) {
        return;
    }
    napi_release_threadsafe_function(g_sync_native_state->tsfn, napi_tsfn_abort);
    napi_release_threadsafe_function(g_sync_native_state->tsfn, napi_tsfn_abort);
    g_sync_native_state->tsfn = nullptr;
}

// 在 ArkTS 线程准备 TSFN / 工作线程。只在首次提交时创建，后续复用。
static bool SyncNativeWorkerEnsure(napi_env env) {
    if (g_sync_native_env != nullptr && g_sync_native_env != env) {
        // 一个 HarmonyOS 模块实例只有一个 ArkTS 环境；拒绝跨环境复用，
        // 避免把 deferred 投递到已销毁的 runtime。
        OH_LOG_ERROR(LOG_APP, "SyncNative: worker already bound to another napi env");
        return false;
    }
    g_sync_native_env = env;

    if (!g_sync_native_cleanup_installed) {
        if (napi_add_env_cleanup_hook(env, SyncNativeCleanupHook, nullptr) != napi_ok) {
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to install env cleanup hook");
            return false;
        }
        g_sync_native_cleanup_installed = true;
    }

    if (g_sync_native_state == nullptr) {
        g_sync_native_state = std::make_shared<SyncNativeWorkerState>();
    }

    if (g_sync_native_state->tsfn == nullptr) {
        napi_value resource_name = nullptr;
        if (napi_create_string_utf8(env, "SujianSyncNativeWorker", NAPI_AUTO_LENGTH,
                                    &resource_name) != napi_ok) {
            return false;
        }

        napi_status status = napi_create_threadsafe_function(
            env, nullptr, nullptr, resource_name,
            0,     // max_queue_size: 0 = 不限制，避免大文件同步时投递排队失败
            1,     // initial_thread_count: 工作线程
            nullptr, nullptr, nullptr,
            SyncNativeCallJs, &g_sync_native_state->tsfn);
        if (status != napi_ok) {
            g_sync_native_state->tsfn = nullptr;
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to create threadsafe function: %{public}d",
                         static_cast<int>(status));
            return false;
        }

        // 模块自身也持有一个引用，保证 cleanup 时可以先 abort 再等工作线程退出。
        if (napi_acquire_threadsafe_function(g_sync_native_state->tsfn) != napi_ok) {
            napi_release_threadsafe_function(g_sync_native_state->tsfn, napi_tsfn_abort);
            g_sync_native_state->tsfn = nullptr;
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to acquire threadsafe function");
            return false;
        }
    }

    if (!g_sync_native_thread_started) {
        try {
            g_sync_native_thread = std::thread(SyncNativeWorkerLoop, g_sync_native_state);
            g_sync_native_thread_started = true;
        } catch (const std::exception& error) {
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start worker thread: %{public}s", error.what());
            SyncNativeDiscardTsfn();
            return false;
        } catch (...) {
            OH_LOG_ERROR(LOG_APP, "SyncNative: failed to start worker thread");
            SyncNativeDiscardTsfn();
            return false;
        }
    }

    return true;
}

static bool SyncNativeWorkerEnqueue(SyncNativeTask* task) {
    std::shared_ptr<SyncNativeWorkerState> state = g_sync_native_state;
    if (state == nullptr) {
        return false;
    }

    std::lock_guard<std::mutex> lock(state->mutex);
    if (state->stopping) {
        return false;
    }
    state->queue.push_back(task);
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

    if (!SyncNativeWorkerEnsure(env)) {
        SyncNativeResolveEnvelope(env, deferred, kSyncNativeCreateFailedEnvelope);
        return promise;
    }

    SyncNativeTask* task = new SyncNativeTask();
    task->deferred = deferred;
    task->execute = execute;
    task->work_name = work_name;

    if (!SyncNativeWorkerEnqueue(task)) {
        delete task;
        SyncNativeResolveEnvelope(env, deferred, kSyncNativeQueueFailedEnvelope);
    }
    return promise;
}

#endif  // SUJIAN_SYNC_NATIVE_WORKER_H
