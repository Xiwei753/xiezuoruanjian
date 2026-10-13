use cpp::cpp;

// ── Qt 文本布局模块：QTextLayout/QTextLine generation cache ──
//
// 线程安全：`g_layout_generations` 为进程级 static，仅在 GUI 线程中使用，
// 不跨线程共享。generation 隔离静态正文和动画/IME 布局。

cpp! {{
    #include <QtGlobal>
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <vector>

    // ── Issue #658: 段落布局缓存 ──
    // layout 阶段（editor_prepare_paragraph_visual_snapshot）创建 QTextLayout 后
    // 存入缓存，rebuild 阶段（rebuild_text_node_from_paragraphs）读取已排好的
    // layout，不在 Scene Graph 阶段重新 beginLayout/createLine/endLayout。
    // 该变量在 editor_prepare_paragraph_visual_snapshot 中使用后由
    // rebuild_text_node_from_paragraphs 读取，两者在 GUI 线程上严格串行。
    //
    // Issue #658 评论 5620035970 问题 2: 用 generation 隔离静态正文和动画/IME 布局，
    // 两条路径各自分配独立 generation，写到独立 generation 的 cache，互不清空。
    // 渲染时用 (generation, slot) 查找 layout。
    struct LayoutGeneration {
        uint64_t generation;
        std::vector<QTextLayout*> layouts;
        // Issue #901 第二十七轮复核问题2: 按 generation 隔离的 freelist 和槽计数器。
        // 释放的槽号回收到本 generation 的 free_slots，新段优先从 free_slots 复用，
        // 否则用 next_slot 分配新槽。跨 generation 的槽号互不影响，不会互相覆盖。
        std::vector<int> free_slots;
        int next_slot;
    };
    static std::vector<LayoutGeneration> g_layout_generations;
    static uint64_t g_next_generation = 1;
    // Issue #658 评论 5621512329 问题 1: 不再用固定数量阈值强制淘汰最老 generation。
    // generation 生命周期由持有方显式管理：
    // - 静态正文：EditorLayout 持有 current_generation，invalidate/snapshot 失效时 clear。
    // - 动画/IME 临时 generation：build_editor_layout_snapshot /
    //   build_virtual_layout_snapshot / record_visual_transaction 在提取完
    //   canonical line/image/cursor 数据后显式 clear_layout_generation 释放。
    // 这样避免仍被静态正文引用的 QTextLayout 被提前删掉。

    static LayoutGeneration* find_layout_generation(uint64_t gen) {
        for (auto& lg : g_layout_generations) {
            if (lg.generation == gen) {
                return &lg;
            }
        }
        return nullptr;
    }

    static LayoutGeneration* ensure_layout_generation(uint64_t gen) {
        auto* existing = find_layout_generation(gen);
        if (existing) return existing;
        g_layout_generations.push_back(LayoutGeneration{gen, {}, {}, 0});
        return &g_layout_generations.back();
    }

    uint64_t begin_layout_generation() {
        return g_next_generation++;
    }

    void clear_layout_generation(uint64_t gen) {
        for (auto it = g_layout_generations.begin(); it != g_layout_generations.end(); ++it) {
            if (it->generation == gen) {
                for (auto* l : it->layouts) {
                    delete l;
                }
                g_layout_generations.erase(it);
                return;
            }
        }
    }

    void clear_all_layout_generations() {
        for (auto& lg : g_layout_generations) {
            for (auto* l : lg.layouts) {
                delete l;
            }
        }
        g_layout_generations.clear();
    }

    QTextLayout* get_paragraph_layout(uint64_t gen, int slot) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return nullptr;
        auto& layouts = lg->layouts;
        if (slot < 0 || slot >= (int)layouts.size()) return nullptr;
        return layouts[slot];
    }

    // Issue #658 评论 5621512329 问题 2: 光标/命中不再重新 new QTextLayout 重新排版，
    // 直接从已排好的 generation cache 中取 QTextLine 调用 cursorToX / xToCursor。
    // gen/slot 对应的 layout 由 EditorLayout 生命周期管理（invalidate 时 clear），
    // 调用方保证 snapshot.layout_generation 在调用时有效。
    double get_paragraph_layout_cursor_to_x_on_line(
        uint64_t gen, int slot, int qline, int cursor_qchar, bool use_trailing
    ) {
        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return 0.0;
        if (qline < 0 || qline >= layout->lineCount()) return 0.0;
        QTextLine line = layout->lineAt(qline);
        if (!line.isValid()) return 0.0;
        int line_start = line.textStart();
        int line_end = line_start + line.textLength();
        int pos = cursor_qchar;
        if (pos < line_start) pos = line_start;
        if (pos > line_end) pos = line_end;
        return line.cursorToX(pos, use_trailing ? QTextLine::Trailing : QTextLine::Leading) - line.x();
    }

    int get_paragraph_layout_x_to_cursor_on_line(
        uint64_t gen, int slot, int qline, double x
    ) {
        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return 0;
        if (qline < 0 || qline >= layout->lineCount()) return 0;
        QTextLine line = layout->lineAt(qline);
        if (!line.isValid()) return 0;
        int line_start = line.textStart();
        int line_end = line_start + line.textLength();
        int pos = line.xToCursor(x + line.x());
        if (pos < line_start) pos = line_start;
        if (pos > line_end) pos = line_end;
        return pos;
    }

    // Issue #658 评论 5620035970 问题 1+2: 按 generation + cache_slot 写指定位置，
    // 不再 push_back。空段落也占一个明确的 null slot，
    // 保持 cache_slot 与文档段落一一对应，避免越界访问。
    void set_paragraph_layout_slot_gen(uint64_t gen, int cache_slot, QTextLayout* layout) {
        auto* lg = ensure_layout_generation(gen);
        auto& layouts = lg->layouts;
        while ((int)layouts.size() <= cache_slot) {
            layouts.push_back(nullptr);
        }
        if (layouts[cache_slot]) {
            delete layouts[cache_slot];
        }
        layouts[cache_slot] = layout;
    }

    void set_null_paragraph_layout_slot_gen(uint64_t gen, int cache_slot) {
        auto* lg = ensure_layout_generation(gen);
        auto& layouts = lg->layouts;
        while ((int)layouts.size() <= cache_slot) {
            layouts.push_back(nullptr);
        }
        if (layouts[cache_slot]) {
            delete layouts[cache_slot];
        }
        layouts[cache_slot] = nullptr;
    }

    // Issue #901 第二十五轮复核问题1: 释放指定 (generation, slot) 的 QTextLayout。
    // 删除该槽的 QTextLayout 并置 nullptr，并把 slot 回收到同一 generation 的 free_slots
    // 供复用。与 set_null_paragraph_layout_slot_gen 的区别：set_null 会 ensure_layout_generation
    // 创建空数组（用于占位），release 只在 generation 已存在时释放，不创建新数组
    //（用于回收已无持有者的旧 layout）。
    //
    // Issue #901 第二十七轮复核问题2: freelist 按 generation 隔离，不再用全局 g_slot_freelist。
    // 释放的 slot 回收到同一 generation 的 free_slots，跨 generation 的槽号互不影响。

    void release_paragraph_layout_slot_gen(uint64_t gen, int slot) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return;
        auto& layouts = lg->layouts;
        if (slot < 0 || slot >= (int)layouts.size()) return;
        if (layouts[slot]) {
            delete layouts[slot];
            layouts[slot] = nullptr;
        }
        // Issue #901 第二十七轮复核问题2: 把 slot 回收到同一 generation 的 free_slots。
        // 防重复回收：检查该 slot 不已在 free_slots 中。
        bool already_free = false;
        for (int s : lg->free_slots) {
            if (s == slot) { already_free = true; break; }
        }
        if (!already_free) {
            lg->free_slots.push_back(slot);
        }
    }

    // Issue #901 第二十七轮复核问题2: 为指定 generation 分配一个物理槽号。
    // 优先从该 generation 的 free_slots 复用已释放的槽（要求 >= min_slot，
    // 避免与同 generation 中复用段保留的 cache_slot 冲突）；
    // 否则用 next_slot 分配新槽（确保 next_slot >= min_slot）。
    int acquire_physical_slot_for_generation(uint64_t gen, int min_slot) {
        auto* lg = ensure_layout_generation(gen);
        // 先从 free_slots 找一个 >= min_slot 的槽。
        for (auto it = lg->free_slots.begin(); it != lg->free_slots.end(); ++it) {
            if (*it >= min_slot) {
                int slot = *it;
                lg->free_slots.erase(it);
                return slot;
            }
        }
        // free_slots 中没有合适的，用 next_slot 分配新槽。
        if (lg->next_slot < min_slot) {
            lg->next_slot = min_slot;
        }
        int slot = lg->next_slot;
        lg->next_slot += 1;
        return slot;
    }

    // 兼容旧调用：clear_paragraph_layout_cache 清全部 generation。
    void clear_paragraph_layout_cache() {
        clear_all_layout_generations();
    }
}}

/// Issue #658 评论 5620035970 问题 2: 分配新的布局 generation。
///
/// 封装 C++ `begin_layout_generation()`，供 layout_ops.rs / pipeline.rs 调用，
/// 避免在非平台封装目录直接调 cpp!(unsafe)。
/// SAFETY: GUI thread only; begin_layout_generation 在 GUI 线程分配新 generation。
pub fn begin_layout_generation() -> u64 {
    cpp!(unsafe [] -> u64 as "uint64_t" {
        return begin_layout_generation();
    })
}

/// Issue #658 评论 5620035970 问题 2: 释放指定 generation 的布局缓存。
///
/// 封装 C++ `clear_layout_generation(gen)`，供外部调用方在 snapshot 失效时释放旧 generation。
/// SAFETY: GUI thread only; gen 是之前 begin_layout_generation 分配的。
pub fn clear_layout_generation(gen: u64) {
    cpp!(unsafe [gen as "uint64_t"] {
        clear_layout_generation(gen);
    });
}

/// Issue #658 评论 5621512329 问题 2: 从已排好的 generation cache 中取 QTextLine，
/// 调用 cursorToX，不再重新 new QTextLayout 重新排版。
///
/// `gen` / `slot` 定位段落 layout，`qline` 是段落内的视觉行索引，
/// `cursor_qchar` 是段落内的 QChar (UTF-16) offset。
/// 返回该 cursor 在行内的 x 坐标（行局部坐标，不含行 x 偏移）。
/// 若 generation/slot/layout 不存在则返回 0.0。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理，
/// 调用方保证 snapshot.layout_generation 在调用时有效。
pub fn get_paragraph_layout_cursor_to_x_on_line(
    gen: u64,
    slot: i32,
    qline: i32,
    cursor_qchar: i32,
    use_trailing: bool,
) -> f64 {
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        qline as "int",
        cursor_qchar as "int",
        use_trailing as "bool"
    ] -> f64 as "double" {
        return get_paragraph_layout_cursor_to_x_on_line(gen, slot, qline, cursor_qchar, use_trailing);
    })
}

/// Issue #658 评论 5621512329 问题 2: 从已排好的 generation cache 中取 QTextLine，
/// 调用 xToCursor，不再重新 new QTextLayout 重新排版。
///
/// `gen` / `slot` 定位段落 layout，`qline` 是段落内的视觉行索引，
/// `x` 是行内 x 坐标（行局部坐标，不含行 x 偏移）。
/// 返回该 x 对应的 QChar (UTF-16) offset（段落内）。
/// 若 generation/slot/layout 不存在则返回 0。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理，
/// 调用方保证 snapshot.layout_generation 在调用时有效。
pub fn get_paragraph_layout_x_to_cursor_on_line(gen: u64, slot: i32, qline: i32, x: f64) -> i32 {
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        qline as "int",
        x as "double"
    ] -> i32 as "int" {
        return get_paragraph_layout_x_to_cursor_on_line(gen, slot, qline, x);
    })
}

/// Issue #901 第二十五轮复核问题1 + 第二十七轮复核问题2: 释放指定 (generation, slot) 的 QTextLayout。
///
/// 删除该槽的 QTextLayout 并将槽置 nullptr，同时把 slot 回收到**同一 generation**
/// 的 free_slots 供后续复用。跨 generation 的槽号互不影响，不会互相覆盖。
/// 用于回收增量编辑中已无持有者的旧 layout（普通编辑改段、拆段、合段后
/// 旧 `cache_slot` 被置 -1，对应的 QTextLayout 不再被任何 snapshot 引用）。
/// SAFETY: GUI thread only; gen/slot 应是之前 set_paragraph_layout_slot_gen 写入的。
pub fn release_paragraph_layout_slot_gen(gen: u64, slot: i32) {
    cpp!(unsafe [gen as "uint64_t", slot as "int"] {
        release_paragraph_layout_slot_gen(gen, slot);
    });
}

/// Issue #901 第二十七轮复核问题2: 为指定 generation 分配一个物理槽号。
///
/// freelist 按 generation 隔离管理（存储在 C++ `LayoutGeneration` 结构体中）。
/// 优先从该 generation 的 `free_slots` 复用已释放的槽（要求 `>= min_slot`，
/// 避免与同 generation 中复用段保留的 `cache_slot` 冲突）；
/// 否则用该 generation 的 `next_slot` 分配新槽。
///
/// `min_slot` 是调用方已知的最小安全槽号（通常为 `next_physical_slot`），
/// 确保新分配的槽不与同 generation 中复用段保留的旧 `cache_slot` 冲突。
///
/// SAFETY: GUI thread only; gen 应是之前 begin_layout_generation 分配的。
pub fn acquire_physical_slot_for_generation(gen: u64, min_slot: i32) -> i32 {
    cpp!(unsafe [gen as "uint64_t", min_slot as "int"] -> i32 as "int" {
        return acquire_physical_slot_for_generation(gen, min_slot);
    })
}
