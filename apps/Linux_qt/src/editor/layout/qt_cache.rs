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

    static std::vector<QTextLayout*>* find_layout_generation(uint64_t gen) {
        for (auto& lg : g_layout_generations) {
            if (lg.generation == gen) {
                return &lg.layouts;
            }
        }
        return nullptr;
    }

    static std::vector<QTextLayout*>* ensure_layout_generation(uint64_t gen) {
        auto* existing = find_layout_generation(gen);
        if (existing) return existing;
        g_layout_generations.push_back(LayoutGeneration{gen, {}});
        return &g_layout_generations.back().layouts;
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
        auto* layouts = find_layout_generation(gen);
        if (!layouts) return nullptr;
        if (slot < 0 || slot >= (int)layouts->size()) return nullptr;
        return (*layouts)[slot];
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
        auto* layouts = ensure_layout_generation(gen);
        while ((int)layouts->size() <= cache_slot) {
            layouts->push_back(nullptr);
        }
        if ((*layouts)[cache_slot]) {
            delete (*layouts)[cache_slot];
        }
        (*layouts)[cache_slot] = layout;
    }

    void set_null_paragraph_layout_slot_gen(uint64_t gen, int cache_slot) {
        auto* layouts = ensure_layout_generation(gen);
        while ((int)layouts->size() <= cache_slot) {
            layouts->push_back(nullptr);
        }
        if ((*layouts)[cache_slot]) {
            delete (*layouts)[cache_slot];
        }
        (*layouts)[cache_slot] = nullptr;
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
