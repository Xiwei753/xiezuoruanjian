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
        // Issue #901 第二十八轮复核问题3: 引用计数 — 与 layouts 一一对应。
        // set_paragraph_layout_slot_gen 写入 layout 时 refcount=1（Store 持有初始引用）。
        // 消费者（snapshot/动画帧）通过 add_layout_ref 增加引用，
        // release_layout_ref 减少引用，refcount 归零时 delete layout 并回收 slot。
        std::vector<int> refcounts;
        // Issue #901 第二十八轮复核问题3: epoch — 与 layouts 一一对应。
        // 同一物理槽被复用时 epoch 递增，旧帧持有的旧 epoch 不匹配，
        // release_layout_ref 时不会误减新 layout 的 refcount。
        std::vector<uint64_t> epochs;
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
        g_layout_generations.push_back(LayoutGeneration{gen, {}, {}, 0, {}, {}});
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

    // Issue #901 第二十九轮复核阻塞3: 带 epoch 校验的 get。
    // epoch 不匹配（slot 已被复用，epoch 递增）返回 nullptr，
    // 防止旧帧误取同槽新对象。epoch 匹配则返回 layouts[slot]。
    QTextLayout* get_paragraph_layout_checked(uint64_t gen, int slot, uint64_t epoch) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return nullptr;
        auto& layouts = lg->layouts;
        auto& epochs = lg->epochs;
        if (slot < 0 || slot >= (int)layouts.size()) return nullptr;
        if (epochs[slot] != epoch) return nullptr;
        return layouts[slot];
    }

    // Issue #901 第二十九轮复核阻塞2+3: 读取指定 (gen, slot) 的当前 epoch。
    // 用于 acquire 后 Store 记录 epoch 到 entry.layout_epoch，
    // 使后续 release_layout_ref(gen, slot, epoch) 能正确校验。
    uint64_t get_layout_epoch(uint64_t gen, int slot) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return 0;
        auto& epochs = lg->epochs;
        if (slot < 0 || slot >= (int)epochs.size()) return 0;
        return epochs[slot];
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

    // Issue #901 第三十轮问题1: 带 epoch 校验的 cursorToX。
    // 通过 get_paragraph_layout_checked 校验 epoch，防止旧帧误取同槽新对象。
    // epoch 不匹配或 layout 不存在时返回 0.0。
    double get_paragraph_layout_cursor_to_x_on_line_checked(
        uint64_t gen, int slot, uint64_t epoch, int qline, int cursor_qchar, bool use_trailing
    ) {
        QTextLayout* layout = get_paragraph_layout_checked(gen, slot, epoch);
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

    // Issue #901 第三十轮问题1: 带 epoch 校验的 xToCursor。
    // 通过 get_paragraph_layout_checked 校验 epoch，防止旧帧误取同槽新对象。
    // epoch 不匹配或 layout 不存在时返回 0。
    int get_paragraph_layout_x_to_cursor_on_line_checked(
        uint64_t gen, int slot, uint64_t epoch, int qline, double x
    ) {
        QTextLayout* layout = get_paragraph_layout_checked(gen, slot, epoch);
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
    // Issue #901 第二十八轮复核问题3: 写入 layout 时设置 refcount=1（Store 持有初始引用），
    // epoch 保持 acquire 时已设的值（freelist 复用时已递增，新槽时为 0）。
    // Issue #901 第二十九轮复核阻塞2: 不再无条件覆盖已有其他持有者的槽。
    // 如果 refcount>0（有其他持有者），不 delete 旧 layout、不重置 refcount，
    // 删除传入的新 layout 避免泄漏，保留旧 layout。调用方应改用
    // install_layout_slot_gen（唯一允许写入 layout 的入口）。
    void set_paragraph_layout_slot_gen(uint64_t gen, int cache_slot, QTextLayout* layout) {
        auto* lg = ensure_layout_generation(gen);
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        while ((int)layouts.size() <= cache_slot) {
            layouts.push_back(nullptr);
            refcounts.push_back(0);
            epochs.push_back(0);
        }
        // 槽被其他持有者占用，不覆盖，删除新 layout 避免泄漏。
        if (refcounts[cache_slot] > 0) {
            delete layout;
            return;
        }
        if (layouts[cache_slot]) {
            delete layouts[cache_slot];
        }
        layouts[cache_slot] = layout;
        // Store 持有初始引用。
        refcounts[cache_slot] = 1;
    }

    // Issue #901 第二十九轮复核阻塞1+2: 在 acquire 已分配的槽上安装 layout。
    // 这是唯一允许写入 layout 的入口。要求槽已由 acquire_physical_slot_for_generation
    // 分配（refcount==0, epoch 已设）。
    // - refcount==0：安全安装，写入 layout，设 refcount=1（Store 初始引用），
    //   保留 acquire 时设的 epoch。
    // - refcount>0：槽被其他持有者占用，不安装，删除新 layout 避免泄漏。
    //   调用方应先 release 再 install。
    void install_layout_slot_gen(uint64_t gen, int slot, QTextLayout* layout) {
        auto* lg = find_layout_generation(gen);
        if (!lg) {
            // generation 不存在不应发生（acquire 应已 ensure）。
            // 为安全起见 ensure 并继续。
            lg = ensure_layout_generation(gen);
        }
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        while ((int)layouts.size() <= slot) {
            layouts.push_back(nullptr);
            refcounts.push_back(0);
            epochs.push_back(0);
        }
        // 槽被其他持有者占用，不安装，删除新 layout 避免泄漏。
        if (refcounts[slot] > 0) {
            delete layout;
            return;
        }
        // refcount==0，安全安装。
        if (layouts[slot]) {
            delete layouts[slot];
        }
        layouts[slot] = layout;
        refcounts[slot] = 1;  // Store 持有初始引用
        // epoch 保持 acquire 时设的值（freelist 复用时已递增，新槽时为 0）。
    }

    // Issue #901 第三十轮问题2: deprecated — 空段不再分配物理槽。
    // full.rs 中空段不再调用 prepare_paragraph_visual_snapshot（cache_slot 传 -1），
    // 此函数保留兼容但不应再被触发。空段没有 QTextLayout 对象，不需要占位 null slot。
    void set_null_paragraph_layout_slot_gen(uint64_t gen, int cache_slot) {
        auto* lg = ensure_layout_generation(gen);
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        while ((int)layouts.size() <= cache_slot) {
            layouts.push_back(nullptr);
            refcounts.push_back(0);
            epochs.push_back(0);
        }
        if (layouts[cache_slot]) {
            delete layouts[cache_slot];
        }
        layouts[cache_slot] = nullptr;
        // Issue #901 第二十八轮复核问题3: null slot 无 layout，refcount 置 0。
        refcounts[cache_slot] = 0;
    }

    // Issue #901 第二十八轮复核问题3: 增加布局引用计数，返回当前 epoch。
    // 用于 snapshot/动画帧持有额外引用。调用方应保存返回的 epoch，
    // 释放时传给 release_layout_ref(gen, slot, epoch)。
    // Issue #901 第三十轮问题3: 此函数不校验传入 epoch，也不验证 slot 有有效布局，
    // 已被 retain_layout_ref 替代。保留仅供 deprecated 兼容。
    uint64_t add_layout_ref(uint64_t gen, int slot) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return 0;
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        if (slot < 0 || slot >= (int)layouts.size()) return 0;
        refcounts[slot] += 1;
        return epochs[slot];
    }

    // Issue #901 第三十轮问题3: 安全的引用计数增加 — 校验 epoch 且要求 slot 有有效布局。
    // 只有 epoch 完全一致且 layouts[slot] 非空才增加引用，返回 true。
    // epoch 不匹配（slot 已被复用，epoch 递增）或 slot 无有效布局时返回 false，不增加引用。
    // 这取代了 add_layout_ref 的不安全行为：add_layout_ref 只根据裸 gen/slot 加引用，
    // 既不验证传入的旧 epoch，也不要求 layouts[slot] 非空，导致旧帧误绑定到新文字。
    bool retain_layout_ref(uint64_t gen, int slot, uint64_t expected_epoch) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return false;
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        if (slot < 0 || slot >= (int)layouts.size()) return false;
        // epoch 不匹配：旧帧试图 retain 已被复用的槽，拒绝。
        if (epochs[slot] != expected_epoch) return false;
        // slot 无有效布局：拒绝。
        if (!layouts[slot]) return false;
        refcounts[slot] += 1;
        return true;
    }

    // Issue #901 第二十八轮复核问题3: 减少布局引用计数，带 epoch 校验。
    // - epoch 不匹配：旧帧释放旧 handle（slot 已被复用，epoch 递增），直接忽略，
    //   不减 refcount，不 delete。
    // - epoch 匹配：减 refcount。refcount 归零时 delete layout、置 nullptr、回收 slot。
    void release_layout_ref(uint64_t gen, int slot, uint64_t epoch) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return;
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        if (slot < 0 || slot >= (int)layouts.size()) return;
        // epoch 不匹配：旧帧释放旧 handle，忽略。
        if (epochs[slot] != epoch) return;
        // epoch 匹配，减 refcount。
        if (refcounts[slot] > 0) {
            refcounts[slot] -= 1;
        }
        if (refcounts[slot] == 0 && layouts[slot]) {
            delete layouts[slot];
            layouts[slot] = nullptr;
            // 把 slot 回收到同一 generation 的 free_slots。
            // 防重复回收：检查该 slot 不已在 free_slots 中。
            bool already_free = false;
            for (int s : lg->free_slots) {
                if (s == slot) { already_free = true; break; }
            }
            if (!already_free) {
                lg->free_slots.push_back(slot);
            }
        }
    }

    // Issue #901 第二十五轮复核问题1: 释放指定 (generation, slot) 的 QTextLayout。
    // 删除该槽的 QTextLayout 并置 nullptr，并把 slot 回收到同一 generation 的 free_slots
    // 供复用。与 set_null_paragraph_layout_slot_gen 的区别：set_null 会 ensure_layout_generation
    // 创建空数组（用于占位），release 只在 generation 已存在时释放，不创建新数组
    //（用于回收已无持有者的旧 layout）。
    //
    // Issue #901 第二十七轮复核问题2: freelist 按 generation 隔离，不再用全局 g_slot_freelist。
    // 释放的 slot 回收到同一 generation 的 free_slots，跨 generation 的槽号互不影响。
    //
    // Issue #901 第二十八轮复核问题3: 改为基于引用计数释放。用当前 epoch 调用
    // release_layout_ref，减 refcount，归零时才 delete + 回收 slot。
    void release_paragraph_layout_slot_gen(uint64_t gen, int slot) {
        auto* lg = find_layout_generation(gen);
        if (!lg) return;
        auto& layouts = lg->layouts;
        if (slot < 0 || slot >= (int)layouts.size()) return;
        // 用当前 epoch 释放 Store 的引用。
        uint64_t cur_epoch = lg->epochs[slot];
        release_layout_ref(gen, slot, cur_epoch);
    }

    // Issue #901 第二十八轮复核问题2: 为指定 generation 分配一个物理槽号。
    // 优先从该 generation 的 free_slots 复用任意一个已释放的槽（不再要求 >= min_slot，
    // 同一代内不会有两个段落同时使用同一 slot，因为 Rust 端只在 cache_slot < 0 新段时调用分配，
    // 复用段保持已有 cache_slot）；
    // 否则用 next_slot 分配新槽。
    // Issue #901 第二十八轮复核问题3: 从 freelist 复用时递增 epoch，旧帧持有的旧 epoch
    // 不匹配，release_layout_ref 时不会误减新 layout 的 refcount。新槽时 epoch=0, refcount=0。
    int acquire_physical_slot_for_generation(uint64_t gen) {
        auto* lg = ensure_layout_generation(gen);
        auto& layouts = lg->layouts;
        auto& refcounts = lg->refcounts;
        auto& epochs = lg->epochs;
        // 优先从 free_slots 复用任意一个已释放的槽。用 back()/pop_back() O(1) 取出。
        if (!lg->free_slots.empty()) {
            int slot = lg->free_slots.back();
            lg->free_slots.pop_back();
            // epoch 递增：旧帧持有的旧 epoch 不匹配，不会误取/误释新布局。
            epochs[slot] += 1;
            // freelist 中的 slot refcount 应已为 0，复用时保持 0，
            // 后续 set_paragraph_layout_slot_gen 会设为 1。
            refcounts[slot] = 0;
            return slot;
        }
        // free_slots 为空，用 next_slot 分配新槽。
        int slot = lg->next_slot;
        lg->next_slot += 1;
        // 扩展 layouts/refcounts/epochs 向量到新槽位置。
        while ((int)layouts.size() <= slot) {
            layouts.push_back(nullptr);
            refcounts.push_back(0);
            epochs.push_back(0);
        }
        // 新槽 epoch=0, refcount=0（set 时会设 refcount=1）。
        epochs[slot] = 0;
        refcounts[slot] = 0;
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

/// Issue #901 第三十轮问题1: 带 epoch 校验的 cursorToX 查询。
///
/// 通过 C++ `get_paragraph_layout_checked(gen, slot, epoch)` 校验 epoch，
/// 防止旧帧误取同槽新对象。epoch 不匹配或 layout 不存在时返回 0.0。
///
/// `epoch` 为 0 时不校验（退化为旧行为，兼容未记录 epoch 的旧路径）。
///
/// SAFETY: GUI thread only; 查询不修改 cache 状态。
pub fn get_paragraph_layout_cursor_to_x_on_line_checked(
    gen: u64,
    slot: i32,
    epoch: u64,
    qline: i32,
    cursor_qchar: i32,
    use_trailing: bool,
) -> f64 {
    if epoch == 0 {
        // 兼容旧路径：epoch 未记录，退化为不校验。
        return get_paragraph_layout_cursor_to_x_on_line(gen, slot, qline, cursor_qchar, use_trailing);
    }
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        epoch as "uint64_t",
        qline as "int",
        cursor_qchar as "int",
        use_trailing as "bool"
    ] -> f64 as "double" {
        return get_paragraph_layout_cursor_to_x_on_line_checked(gen, slot, epoch, qline, cursor_qchar, use_trailing);
    })
}

/// Issue #901 第三十轮问题1: 带 epoch 校验的 xToCursor 查询。
///
/// 通过 C++ `get_paragraph_layout_checked(gen, slot, epoch)` 校验 epoch，
/// 防止旧帧误取同槽新对象。epoch 不匹配或 layout 不存在时返回 0。
///
/// `epoch` 为 0 时不校验（退化为旧行为，兼容未记录 epoch 的旧路径）。
///
/// SAFETY: GUI thread only; 查询不修改 cache 状态。
pub fn get_paragraph_layout_x_to_cursor_on_line_checked(
    gen: u64,
    slot: i32,
    epoch: u64,
    qline: i32,
    x: f64,
) -> i32 {
    if epoch == 0 {
        // 兼容旧路径：epoch 未记录，退化为不校验。
        return get_paragraph_layout_x_to_cursor_on_line(gen, slot, qline, x);
    }
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        epoch as "uint64_t",
        qline as "int",
        x as "double"
    ] -> i32 as "int" {
        return get_paragraph_layout_x_to_cursor_on_line_checked(gen, slot, epoch, qline, x);
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

/// Issue #901 第二十八轮复核问题2: 为指定 generation 分配一个物理槽号。
///
/// freelist 按 generation 隔离管理（存储在 C++ `LayoutGeneration` 结构体中）。
/// 优先从该 generation 的 `free_slots` 复用任意一个已释放的槽（不再要求 `>= min_slot`，
/// 同一代内不会有两个段落同时使用同一 slot，因为 Rust 端只在 `cache_slot < 0` 新段时
/// 调用分配，复用段保持已有 `cache_slot`）；否则用该 generation 的 `next_slot` 分配新槽。
///
/// SAFETY: GUI thread only; gen 应是之前 begin_layout_generation 分配的。
pub fn acquire_physical_slot_for_generation(gen: u64) -> i32 {
    cpp!(unsafe [gen as "uint64_t"] -> i32 as "int" {
        return acquire_physical_slot_for_generation(gen);
    })
}

/// Issue #901 第二十八轮复核问题3: 增加布局引用计数，返回当前 epoch。
///
/// 用于 snapshot/动画帧/Store 持有额外引用。调用方应保存返回的 `epoch`，
/// 释放时传给 [`release_layout_ref`]`(gen, slot, epoch)`。
/// 若 generation/slot 不存在则返回 0。
///
/// # Deprecated
///
/// 此函数不校验传入 epoch，也不验证 slot 有有效布局。
/// 新代码应使用 [`retain_layout_ref`] 替代 — 后者要求 epoch 匹配且 slot 有有效布局
/// 才增加引用，防止旧帧误绑定到新文字。
///
/// SAFETY: GUI thread only; gen/slot 应是之前 set_paragraph_layout_slot_gen 写入的。
#[deprecated(note = "使用 retain_layout_ref 替代，后者校验 epoch 且要求 slot 有有效布局")]
pub fn add_layout_ref(gen: u64, slot: i32) -> u64 {
    cpp!(unsafe [gen as "uint64_t", slot as "int"] -> u64 as "uint64_t" {
        return add_layout_ref(gen, slot);
    })
}

/// Issue #901 第三十轮问题3: 安全的引用计数增加 — 校验 epoch 且要求 slot 有有效布局。
///
/// 只有 **epoch 完全一致且 slot 有有效布局** 才增加引用，返回 `true`。
/// epoch 不匹配（slot 已被复用，epoch 递增）或 slot 无有效布局时返回 `false`，
/// 不增加引用。
///
/// 这取代了 [`add_layout_ref`] 的不安全行为：`add_layout_ref` 只根据裸 gen/slot
/// 加引用，既不验证传入的旧 epoch，也不要求 layouts[slot] 非空，导致旧帧误绑定
/// 到新文字。
///
/// # 安全保证
///
/// 旧帧拿 `(gen=1, slot=0, epoch=0)`，Store 将旧对象释放并回收 slot0，
/// 随后新段复用该槽 epoch 变成 1；旧帧这时调用 `retain_layout_ref(1, 0, 0)`，
/// epoch 不匹配（0 != 1），返回 `false`，**不给新文字的布局加引用**。
///
/// SAFETY: GUI thread only; gen/slot/expected_epoch 应是之前
/// acquire_physical_slot_for_generation 分配并 install_layout_slot_gen 安装 layout 的。
pub fn retain_layout_ref(gen: u64, slot: i32, expected_epoch: u64) -> bool {
    cpp!(unsafe [gen as "uint64_t", slot as "int", expected_epoch as "uint64_t"] -> bool as "bool" {
        return retain_layout_ref(gen, slot, expected_epoch);
    })
}

/// Issue #901 第二十八轮复核问题3: 减少布局引用计数，带 epoch 校验。
///
/// - `epoch` 不匹配：旧帧释放旧 handle（slot 已被复用，epoch 递增），直接忽略，
///   不减 refcount，不 delete。
/// - `epoch` 匹配：减 refcount。refcount 归零时 delete layout、置 nullptr、回收 slot
///   到同一 generation 的 freelist 供后续复用。
///
/// 核心安全保证：同一物理槽被复用后 epoch 递增，旧帧持有的旧 epoch 不匹配，
/// 不会误减新 layout 的 refcount，也不会误 delete 新 layout。
///
/// SAFETY: GUI thread only; gen/slot/epoch 应是之前 add_layout_ref 或
/// set_paragraph_layout_slot_gen（隐含 refcount=1, epoch 由 acquire 设置）返回的。
pub fn release_layout_ref(gen: u64, slot: i32, epoch: u64) {
    cpp!(unsafe [gen as "uint64_t", slot as "int", epoch as "uint64_t"] {
        release_layout_ref(gen, slot, epoch);
    });
}

/// Issue #901 第二十九轮复核阻塞3: 读取指定 (gen, slot) 的当前 epoch。
///
/// 用于 acquire 后 Store 记录 epoch 到 `entry.layout_epoch`，
/// 使后续 [`release_layout_ref`]`(gen, slot, epoch)` 能正确校验。
/// 若 generation/slot 不存在则返回 0。
///
/// SAFETY: GUI thread only; gen/slot 应是之前 acquire_physical_slot_for_generation
/// 分配的。
pub fn get_layout_epoch(gen: u64, slot: i32) -> u64 {
    cpp!(unsafe [gen as "uint64_t", slot as "int"] -> u64 as "uint64_t" {
        return get_layout_epoch(gen, slot);
    })
}

/// Issue #901 第二十九轮复核阻塞3: 校验 epoch 并查询 layout 是否存在。
///
/// 通过 C++ `get_paragraph_layout_checked(gen, slot, epoch)` 校验 epoch，
/// 防止旧帧误取同槽新对象。返回 `true` 表示 layout 存在且 epoch 匹配。
///
/// SAFETY: GUI thread only; 查询不修改 cache 状态。
pub fn is_layout_valid(gen: u64, slot: i32, epoch: u64) -> bool {
    cpp!(unsafe [gen as "uint64_t", slot as "int", epoch as "uint64_t"] -> bool as "bool" {
        return get_paragraph_layout_checked(gen, slot, epoch) != nullptr;
    })
}
