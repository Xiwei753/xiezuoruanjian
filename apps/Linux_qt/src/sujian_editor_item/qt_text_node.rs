//! Qt 6.7+ 公开 API QSGTextNode 封装模块。
//!
//! 把自研写作区的静态正文渲染从"整块 QImage -> QSGImageNode"收口到
//! Qt 6.7+ 公开 API `QQuickWindow::createTextNode()` / `QSGTextNode::addTextLayout()`。
//!
//! ## 架构
//!
//! 场景图 child[0] 是 QSGTransformNode（静态正文层），其下挂一个 QSGTextNode。
//! - 正文变更/字体/宽度变化时：从 `EditorLayout::snapshot()` 已排好的 VisualLine
//!   数据构造 per-paragraph QTextLayout，调用 `QSGTextNode::addTextLayout()`，
//!   把 glyph 数据烘焙到节点内部。
//! - 滚动时：只改 QSGTransformNode 的位移矩阵（translate(0, -scroll_y)），
//!   不重新排版，不重新理解文字。
//!
//! ## 线程约束
//!
//! 使用 threaded render loop 时，`QQuickItem::updatePaintNode()` 在 render thread
//! 执行，GUI 线程此时被阻塞。QSGTextNode/QSGNode 的创建和修改留在
//! `updatePaintNode()` 的 render-thread 阶段。正文排版由唯一的 EditorLayout
//! 路线准备，Scene Graph 只消费已准备的数据，不因迁移到 QSGTextNode 又把
//! 排版职责塞回渲染阶段。
//!
//! ## 不变量
//!
//! - 不使用私有头文件 QSGInternalTextNode，只用公开 QSGTextNode。
//! - 不把 C++ 调用散进各文件，都集中在这个模块。
//! - 正文排版只由 EditorLayout（layout.rs）产生唯一 canonical 结果，
//!   本模块只消费已排好的 VisualLine，不再自行创建第二套全文 QTextLayout。
//! - QTextLayout 由 layout 阶段（editor_prepare_paragraph_visual_snapshot）创建，
//!   本模块通过 g_paragraph_layout_cache 读取已排好的 layout，不再 createLine。

use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGTextNode>
    #include <QtQuick/QSGClipNode>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QQuickWindow>
    #include <QtQuick/QQuickItem>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QtGui/QFont>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <QtCore/QPointF>
    #include <QtCore/QMetaObject>
    #include <QtCore/QRunnable>
    #include <QDebug>
    #include <vector>
    #include <algorithm>
    #include <unordered_map>
    #include <unordered_set>

    // Issue #901 第十轮问题4: per-item ParagraphId → 段落容器节点 映射。
    //
    // C++ 静态层持久维护每个 item/session、ParagraphId 对应的 QSG 节点。
    // staticLayer (QSGTransformNode) 下挂 per-paragraph 的段落位置层
    //（QSGTransformNode，矩阵 translate(0, para_y)），段落位置层下挂该段落的
    // QSGTextNode 或 QSGClipNode 集合（段内 y=0 构建）。
    // 增量更新时按 ParagraphId 做 insert/remove/update，不全量重建。
    //
    // Issue #901 第十四轮问题4: key 改为 QSGTransformNode*（staticLayer 指针），
    // 不再用 void*（item_ptr），消除地址复用风险。
    // ItemNodeMap 新增 session_id 字段，切章时 session_id 变化自动清空旧 map。
    struct IncrementalParaNodeInfo {
        // Issue #901 第十三轮问题3: 段落位置层 QSGTransformNode。
        // 矩阵为 translate(0, para_y)，把段内 y=0 的子节点平移到文档坐标。
        // para_y 变化时只更新此矩阵和 markDirty(DirtyMatrix)，不重建子节点。
        QSGTransformNode* transform_node;
        double para_y;       // 段落第一行文档 y（用于排序和 transform 矩阵）
        // Issue #901 第十四轮问题2: per-paragraph 内容版本签名 (content_revision)。
        // 不用全局 generation，而是 per-paragraph 的 text hash。
        // content_revision 变化 → 重建子节点。其他段变化不影响本段判断。
        uint64_t applied_content_revision;
        int applied_cache_idx;
        // Issue #904 评论 2: per-paragraph layout epoch。
        // 内容身份必须同时比较 (content_revision, cache_idx, epoch)：
        // 同槽位可能被新一代 layout 复用，只有 content hash 相同但 epoch 变了
        // 也必须重建，否则会画到旧 layout 的 text node。
        uint64_t applied_epoch;
        // Issue #904 复核 1: 本段当前实际应用的“整体 y 偏移”。
        // 矩阵 y 取 para_y + applied_y_offset，偏移变化也要更新矩阵/标记脏。
        double applied_y_offset;
        // Issue #901 第十三轮问题4+第十四轮问题2: 该段实际相交的 clip signature。
        // 只保存与该段 y 范围相交的 clip（按 visual line y 范围过滤），
        // 而非整页 clip。这样无关 clip 变化不会触发该段重建。
        // clip signature 变化时重建裁剪子节点，内容 QSGTextNode 不动。
        std::vector<std::pair<double,double>> applied_clip_xy;
        std::vector<std::pair<double,double>> applied_clip_wh;
        // 该段实际应用的 visual line qtextline 索引集合。
        std::vector<int> applied_vl_qtextline_idxs;
    };
    // per-item 的映射表 + 所属 staticLayer 指针 + session_id。
    // static_layer 用于检测 QSG 树变更（item 销毁后重建会生成新 staticLayer），
    // 此时旧 map 条目中的 container 指针已悬空，需清空 map 重新开始。
    // session_id 用于检测切章/清文档（session 变化时清空 para_map）。
    // item_ptr 用于 cleanup 函数按 item 查找并清理 map 条目。
    struct ItemNodeMap {
        std::unordered_map<uint64_t, IncrementalParaNodeInfo> para_map;
        QSGTransformNode* static_layer;
        uint64_t session_id;
        void* item_ptr;
        ItemNodeMap() : static_layer(nullptr), session_id(0), item_ptr(nullptr) {}
    };
    // Issue #901 第十四轮问题4: key 改为 QSGTransformNode*（staticLayer 指针）。
    // staticLayer 是 QSG 树节点指针，item 销毁后 QSG 树被回收，
    // 新 item 生成新 QSG 树和新 staticLayer 指针，不会地址复用。
    static std::unordered_map<QSGTransformNode*, ItemNodeMap> g_incremental_para_map;

    // Issue #658: 段落布局缓存 — 由 layout.rs 的 cpp! 块定义，
    // 此处通过 extern 引用同一链接单元中的定义。
    // Issue #658 评论 5620035970 问题 2: 用 generation 隔离，通过 get_paragraph_layout(gen, slot) 查找。
    extern QTextLayout* get_paragraph_layout(uint64_t gen, int slot);
    // Issue #901 第三十轮问题1: 带 epoch 校验的 get，防止旧帧误取同槽新对象。
    extern QTextLayout* get_paragraph_layout_checked(uint64_t gen, int slot, uint64_t epoch);

    /// Issue #658: 从已排好的 per-paragraph 数据重建静态正文 QSGTextNode。
    ///
    /// 通过 cache_idx 在 g_paragraph_layout_cache 中查找已排好的 QTextLayout。
    /// 每个段落的 QTextLayout 在 editor_layout_lines() 阶段创建并缓存，
    /// 且每个 QTextLine 已 setPosition，layout 完整排版。
    ///
    /// 主 textNode 按段落整段 addTextLayout（高效）。
    /// 动画裁剪按视觉行（VisualLineClipInfo），用 QSGTextNode::addTextLayout 的
    /// lineStart/lineCount 参数按单独视觉行绘制，setClipRect 用视觉行 y/h。
    ///
    /// Issue #668 评论 5646458592 问题 1: 改成一次完整的原子替换。
    /// 先创建新的 QSGTextNode，不动当前正在显示的旧节点；先检查所有非空段落的
    /// (generation, cache_slot) 都能取到 QTextLayout*，再向新节点 addTextLayout()。
    /// 任意必需 layout 缺失时，销毁这次新建的节点，旧静态正文节点保持原样，
    /// 返回 false。所有段落都构建成功以后，才把新节点替换进 staticLayer，
    /// 随后删除旧节点。动画裁剪分支同样按"先构建完整新子树，再替换"的顺序。
    /// 返回 true 表示重建成功，false 表示因 layout 缺失而放弃（调用方应保留
    /// dirty 标志，下一帧继续处理正确的 snapshot/generation）。
    bool rebuild_text_node_from_paragraphs(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
        const int* para_start_arr, const int* cache_idx_arr,
        const uint64_t* para_epoch_arr,
        const double* para_y_arr, const double* indent_w_arr,
        const double* doc_width_arr,
        int para_count,
        const int* vl_cache_idx_arr,
        const uint64_t* vl_epoch_arr,
        const int* vl_qtextline_idx_arr,
        const double* vl_y_arr,
        const double* vl_height_arr,
        const double* vl_x_arr,
        const double* vl_width_arr,
        const double* vl_para_y_arr,
        const double* vl_doc_width_arr,
        const bool* vl_has_qt_layout_arr,
        int vl_count,
        float font_size, const QString& font_family,
        double scroll_y, const QString& color_q,
        const double* clip_x_arr, const double* clip_y_arr,
        const double* clip_w_arr, const double* clip_h_arr,
        int clip_count,
        double origin_x,
        uint64_t generation
    ) {
        if (!root_raw || !item_ptr) return false;
        QQuickWindow *window = item_ptr->window();
        if (!window) return false;

        if (root_raw->childCount() == 0) return false;
        auto *staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return false;

        // Issue #668 评论 5646458592 问题 1: 先检查所有非空段落的
        // (generation, cache_slot) 都能取到 QTextLayout*。任意缺失立即返回 false，
        // 不动当前正在显示的旧节点，不留下一个已经 clear() 过的空节点。
        // Issue #901 第三十轮问题1: 用 get_paragraph_layout_checked 校验 epoch，
        // 防止旧帧误取同槽新对象。
        // Issue #901 第三十一轮问题2: epoch=0 是合法第一版，一律走 checked 校验。
        for (int i = 0; i < para_count; i++) {
            int cacheIdx = cache_idx_arr[i];
            // 空段落（text_lens[i] == 0）不需要 layout，跳过。
            if (text_lens[i] == 0) continue;
            uint64_t epoch = para_epoch_arr[i];
            QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
            if (!cachedLayout) {
                return false;
            }
        }
        // 动画裁剪分支也需要检查所有视觉行的 layout 可用性。
        // Issue #901 第三十一轮问题1: 空段视觉行（has_qt_layout == false）跳过
        // layout 查询，不 return false。其高度、段落范围和占位保留。
        if (clip_count > 0 && vl_count > 0) {
            for (int i = 0; i < vl_count; i++) {
                if (!vl_has_qt_layout_arr[i]) continue;
                int cacheIdx = vl_cache_idx_arr[i];
                uint64_t epoch = vl_epoch_arr[i];
                QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
                if (!cachedLayout) {
                    return false;
                }
            }
        }

        // Issue #709 评论 issue-body-709: 静态正文与吐字/吞字动画互斥渲染。
        // 旧实现无论是否有动画裁剪都先创建完整 newTextNode 把整篇正文画出来，
        // 再额外创建 complement clip 节点。两套都挂到 staticLayer 导致：
        // - InsertReveal 要隐藏的新字早就在完整 newTextNode 里画出来了
        // - DeleteConceal 想让旧字慢慢吞掉，但静态正文已经直接画了新的结果
        // - complement clip 只是多画一份，不会把前面那个完整正文兄弟节点裁掉
        // 改成两条互斥路径：
        // - clip_count == 0：创建一个完整正文 QSGTextNode（无动画接管）
        // - clip_count > 0：不创建完整正文节点，只按每条 visual line 的 complement
        //   区间生成 QSGClipNode + QSGTextNode，被动画接管的区域真的从静态层消失。
        //
        // Issue #853 渲染层语义契约（本函数即该契约的实现）:
        // - RenderPlan 把动画层接管的完整 target cluster 作为 clip 传入；对应
        //   glyph 的当前视觉也在同一份计划中。
        // - scene_graph_renderer 先撤掉纹理不可用的 exclusion，再合并同视觉行区间，
        //   最后由本函数计算静态正文的 complement。
        // - 被动画接管的区域必须**从静态正文层隐藏**，再由动画层绘制。本函数
        //   clip_count > 0 分支不创建完整正文节点，只按 complement 区间生成
        //   clip+text 节点——静态层只画 complement（未被动画接管的区域）。
        // - **不允许静态正文先完整画出，再叠一层动画**。旧实现（Issue #709 之前）
        //   先画完整 newTextNode 再叠 complement clip，导致 InsertReveal 的新字
        //   早就在完整正文里画出来了。本函数的互斥路径彻底消除这一类重影。
        QSGTextNode *newTextNode = nullptr;
        QColor textColor(color_q);

        // Issue #668 评论 5646458592 问题 1: 动画裁剪分支同样按"先构建完整新子树，
        // 再替换"的顺序。先在 newTextNode 之外构建所有 clipNode + clipTextNode，
        // 全部成功后再替换进 staticLayer。构建过程中任何失败（layout 缺失已在前面
        // 检查过，此处不会发生）都不破坏现有正文。
        // 用临时 vector 收集所有新建的 clipNode，最后统一挂到 staticLayer。
        struct PendingClipNode {
            QSGClipNode* clipNode;
            QSGTextNode* clipTextNode;
        };
        std::vector<PendingClipNode> pendingClipNodes;

        // Issue #709 评论 issue-body-709: clip_count == 0 分支 — 无动画接管，
        // 创建一个完整正文 QSGTextNode，按段落整段 addTextLayout。
        if (clip_count == 0) {
            newTextNode = window->createTextNode();
            if (!newTextNode) return false;

            // Issue #658: setColor() 必须在第一次 addTextLayout() 之前设置。
            newTextNode->setColor(textColor);

            // Issue #658: 主 textNode — 按段落整段 addTextLayout。
            // cachedLayout 内部每个 QTextLine 已 setPosition（由 editor_layout_lines 或
            // editor_prepare_paragraph_visual_snapshot 设置），addTextLayout 的
            // QPointF(origin_x, y) 偏移段落第一行到文档 (origin_x=padding, y)，
            // 后续行位置由 layout 内部 position 决定。
            // Issue #658 评论 5620035970 问题 4: 正文从 padding 开始画，
            // 与光标/选区/动画的 VisualLine.x = padding + x_off 一致。
            for (int i = 0; i < para_count; i++) {
                int cacheIdx = cache_idx_arr[i];
                double y = para_y_arr[i];

                // Issue #658 评论 5620035970 问题 2: 用 (generation, cacheIdx) 查找 layout，
                // 不再直接索引 g_paragraph_layout_cache，避免与动画/IME 路径互相清空。
                // Issue #668: 前面已检查所有非空段落 layout 可用，此处 cachedLayout 必非 null。
                // Issue #901 第三十轮问题1: 用 get_paragraph_layout_checked 校验 epoch。
                // Issue #901 第三十一轮问题2: epoch=0 是合法第一版，一律走 checked 校验。
                uint64_t epoch = para_epoch_arr[i];
                QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
                if (cachedLayout) {
                    newTextNode->addTextLayout(QPointF(origin_x, y), cachedLayout);
                }
            }

            newTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }

        // Issue #658: 动画裁剪 — 按视觉行使用精确文档 x/y/w/h 裁剪。
        // 不再按段落整段裁（会误裁同行其他文字），不再猜最后段高度（py+30.0）。
        // 对每一视觉行，只处理与该行 y 范围相交的 AnimationClipRect，
        // 计算这一行自己的 x complement，用 QSGTextNode::addTextLayout 的
        // lineStart/lineCount 参数按单独视觉行绘制。
        // Issue #709 评论 issue-body-709: clip_count > 0 分支不创建完整正文节点，
        // 只按 complement 区间生成 clip+text 节点。被 InsertReveal / DeleteConceal /
        // Reflow 接管的区域真的从静态层消失，只让动画层负责。
        // 没被动画影响的行，complement 就是整行可绘制区 [origin_x, doc_width - origin_x]。
        // vl_count == 0 时（无视觉行数据）不画任何静态正文，staticLayer 替换为空，
        // 全部正文交给动画层。
        if (clip_count > 0 && vl_count > 0) {
            struct ClipRect { double x, y, w, h; };
            std::vector<ClipRect> clipRects;
            for (int c = 0; c < clip_count; c++) {
                double cx = clip_x_arr[c];
                double cy = clip_y_arr[c];
                double cw = clip_w_arr[c];
                double ch = clip_h_arr[c];
                if (ch > 0.0 && cw > 0.0) {
                    clipRects.push_back({cx, cy, cw, ch});
                }
            }
            if (!clipRects.empty()) {
                for (int i = 0; i < vl_count; i++) {
                    // Issue #901 第三十一轮问题1: 空段视觉行（has_qt_layout == false）
                    // 跳过 layout 查询，不 continue 整行放弃——此处 continue 只是
                    // 不为该行创建 clip+text 节点，其高度和段落占位由段落容器节点保留。
                    if (!vl_has_qt_layout_arr[i]) continue;
                    int cacheIdx = vl_cache_idx_arr[i];
                    int qline = vl_qtextline_idx_arr[i];
                    double ly = vl_y_arr[i];
                    double lh = vl_height_arr[i];
                    if (lh < 1.0) lh = 1.0;
                    double dw = vl_doc_width_arr[i];
                    double paraY = vl_para_y_arr[i];

                    // Issue #658 评论 5620035970 问题 2: 用 (generation, cacheIdx) 查找 layout。
                    // Issue #668: 前面已检查所有视觉行 layout 可用，此处必非 null。
                    // Issue #901 第三十轮问题1: 用 get_paragraph_layout_checked 校验 epoch。
                    // Issue #901 第三十一轮问题2: epoch=0 是合法第一版，一律走 checked 校验。
                    uint64_t epoch = vl_epoch_arr[i];
                    QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
                    if (!cachedLayout) {
                        continue;
                    }

                    struct XRange { double left, right; };
                    std::vector<XRange> mergedClips;

                    // Issue #658 评论 5620035970 问题 4: complement 横向范围
                    // 按 [origin_x, dw - origin_x] 计算（即 [padding, width - padding]），
                    // 不再默认 [0, dw]，与正文实际绘制范围一致。
                    double contentLeft = origin_x;
                    double contentRight = dw - origin_x;
                    if (contentRight < contentLeft) contentRight = contentLeft;

                    struct TempXClip { double x, x2; };
                    std::vector<TempXClip> xClips;
                    for (const auto& cr : clipRects) {
                        // Issue #658: 只处理与该视觉行 y 范围相交的动画矩形。
                        if (cr.y < ly + lh && cr.y + cr.h > ly) {
                            double crLeft = cr.x;
                            double crRight = cr.x + cr.w;
                            if (crLeft < contentLeft) crLeft = contentLeft;
                            if (crRight > contentRight) crRight = contentRight;
                            if (crLeft < crRight) {
                                xClips.push_back({crLeft, crRight});
                            }
                        }
                    }

                    std::sort(xClips.begin(), xClips.end(),
                        [](const TempXClip& a, const TempXClip& b) { return a.x < b.x; });
                    for (const auto& xc : xClips) {
                        if (!mergedClips.empty() && xc.x <= mergedClips.back().right) {
                            mergedClips.back().right = std::max(mergedClips.back().right, xc.x2);
                        } else {
                            mergedClips.push_back({xc.x, xc.x2});
                        }
                    }

                    std::vector<XRange> complements;
                    // Issue #658 评论 5620035970 问题 4: complement 从 origin_x 开始，
                    // 到 contentRight = dw - origin_x 结束，与正文绘制范围一致。
                    double curX = contentLeft;
                    for (const auto& mc : mergedClips) {
                        if (mc.left > curX) {
                            complements.push_back({curX, mc.left});
                        }
                        curX = mc.right;
                    }
                    if (curX < contentRight) {
                        complements.push_back({curX, contentRight});
                    }

                    for (const auto& comp : complements) {
                        double clipW = comp.right - comp.left;
                        if (clipW < 0.5) continue;

                        auto *clipNode = new QSGClipNode;
                        clipNode->setIsRectangular(true);
                        // Issue #658: setClipRect 用视觉行 y/h，不再用整段 py/ph。
                        clipNode->setClipRect(QRectF(comp.left, ly, clipW, lh));

                        QSGTextNode* clipTextNode = window->createTextNode();
                        if (clipTextNode) {
                            clipTextNode->setColor(textColor);
                            clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                            clipNode->appendChildNode(clipTextNode);

                            // Issue #658 评论 5620035970 问题 3: addTextLayout 真实签名
                            // (position, layout, selectionStart, selectionCount, lineStart, lineCount)。
                            // 之前误把 qline, 1 传给 selectionStart/selectionCount，
                            // 导致只画选中态而非整行。改为 (-1, -1, qline, 1) 表示
                            // 无选区、按 lineStart=qline/lineCount=1 绘制单视觉行。
                            // 问题 4: QPointF(origin_x, paraY) 偏移段落第一行到
                            // (padding, paraY)，与主 textNode 一致。
                            clipTextNode->addTextLayout(QPointF(origin_x, paraY), cachedLayout, -1, -1, qline, 1);
                            clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

                            pendingClipNodes.push_back({clipNode, clipTextNode});
                        } else {
                            // clipTextNode 创建失败，销毁 clipNode，继续处理其他行。
                            delete clipNode;
                        }
                    }
                }
            }
        }

        // Issue #668 评论 5646842592 问题 1: 新子树已完整构建（newTextNode +
        // 所有 pendingClipNodes）。现在执行整棵子树原子替换：移除并删除
        // staticLayer 上的所有旧子节点（旧主 textNode + 所有旧 clipNode），
        // 再统一挂入新子树。此前的任何失败都已 return false 且不破坏旧节点。
        // 旧实现只删 childAtIndex(0) + append 到末尾，当 staticLayer 上一帧
        // 存在动画裁剪（主 QSGTextNode + 多个 QSGClipNode）时，旧 clip 节点
        // 会残留；删 child[0] 后原 child[1] 顶到 child[0]，新 text node append
        // 到末尾，下一次重建 child[0] 甚至可能不是主 text node，出现重复字、
        // 幽灵字和节点顺序错乱。整棵子树原子替换消除这一类问题。
        //
        // Issue #901 第十一轮问题3: 全量 rebuild 会删除 staticLayer 的所有旧子节点，
        // 旧 g_incremental_para_map 条目中的 container 指针在删除后变悬空。
        // 必须在删除子节点**之前**清空该 item 的增量映射条目，否则下一次增量
        // 路径会通过旧 container 指针访问已释放的内存（removeChildNode/delete/
        // childCount），导致崩溃。全量 rebuild 后 map 为空，下一次增量路径从头
        // 建立节点身份表。erase 不会解引用指针，仅按指针值移除 map 条目，安全。
        // Issue #901 第十四轮问题4: key 改为 QSGTransformNode*（staticLayer 指针）。
        g_incremental_para_map.erase(staticLayer);
        while (staticLayer->childCount() > 0) {
            QSGNode *oldChild = staticLayer->childAtIndex(staticLayer->childCount() - 1);
            staticLayer->removeChildNode(oldChild);
            delete oldChild;
        }
        // Issue #709 评论 issue-body-709: 挂入新的子树。
        // - clip_count == 0：挂 newTextNode（完整正文）
        // - clip_count > 0：挂所有 pendingClipNodes（不挂 newTextNode，它为 null）
        if (newTextNode) {
            staticLayer->appendChildNode(newTextNode);
        }
        // 挂入所有动画裁剪 clipNode（clipTextNode 已作为 clipNode 子节点挂好）。
        for (const auto& pcn : pendingClipNodes) {
            staticLayer->appendChildNode(pcn.clipNode);
        }

        // 滚动位移：通过 QSGTransformNode 矩阵平移
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);

        return true;
    }

    /// 滚动帧：只更新 QSGTransformNode 的位移矩阵，不重建静态正文节点。
    ///
    /// 不出现 clear()、QTextLayout、createLine()、addTextLayout()。
    void update_scroll_transform(
        QSGNode* root_raw, double scroll_y
    ) {
        if (!root_raw || root_raw->childCount() == 0) return;
        auto *staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return;

        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);
    }

    /// Issue #901 第五轮阻塞5: 只重建受影响段落的 QSGTextNode，不全文重建。
    ///
    /// staticLayer 的子节点是 per-paragraph QSGTextNode（按段落顺序排列）。
    /// 对 `affected_indices` 中列出的段落，删除旧子节点并创建新 QSGTextNode；
    /// 不在 `affected_indices` 中的段落保持原子节点不变。
    /// 最后更新滚动位移矩阵。
    ///
    /// 返回 true 表示成功更新；false 表示 staticLayer 子节点数与 para_count
    /// 不匹配（需要先调用 rebuild_text_node_from_paragraphs 全量重建）。
    bool update_affected_paragraph_nodes(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
        const int* cache_idx_arr,
        const uint64_t* para_epoch_arr,
        const double* para_y_arr,
        int para_count,
        const int* affected_indices, int affected_count,
        float font_size, const QString& font_family,
        double scroll_y, const QString& color_q,
        double origin_x,
        uint64_t generation
    ) {
        if (!root_raw || !item_ptr) return false;
        QQuickWindow *window = item_ptr->window();
        if (!window) return false;

        if (root_raw->childCount() == 0) return false;
        auto *staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return false;

        // 检查 staticLayer 子节点数是否匹配 para_count
        // 不匹配说明需要全量重建，返回 false
        if (staticLayer->childCount() != para_count) return false;

        QColor textColor(color_q);

        // 只重建受影响段落的 QSGTextNode
        for (int a = 0; a < affected_count; a++) {
            int i = affected_indices[a];
            if (i < 0 || i >= para_count) continue;
            if (i >= staticLayer->childCount()) continue;

            // 空段落不需要 QSGTextNode，但需要占位（保持子节点数一致）
            if (text_lens[i] == 0) continue;

            // 检查 layout 可用性
            // Issue #901 第三十轮问题1: 用 get_paragraph_layout_checked 校验 epoch。
            // Issue #901 第三十一轮问题2: epoch=0 是合法第一版，一律走 checked 校验。
            int cacheIdx = cache_idx_arr[i];
            uint64_t epoch = para_epoch_arr[i];
            QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
            if (!cachedLayout) {
                return false;
            }

            // 获取旧子节点（要替换的）
            QSGNode* oldChild = staticLayer->childAtIndex(i);
            if (!oldChild) continue;

            // 创建新 QSGTextNode
            QSGTextNode* newTextNode = window->createTextNode();
            if (!newTextNode) continue;

            newTextNode->setColor(textColor);
            double y = para_y_arr[i];
            newTextNode->addTextLayout(QPointF(origin_x, y), cachedLayout);
            newTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

            // 原子替换：在旧节点前插入新节点，然后移除并删除旧节点
            staticLayer->insertChildNodeBefore(newTextNode, oldChild);
            staticLayer->removeChildNode(oldChild);
            delete oldChild;
        }

        // 更新滚动位移矩阵
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);

        return true;
    }

    /// Issue #904 评论 2: 为单个段落构建子节点（textNode 或 clipNode 集合），
    /// 成功返回 true。被 update_text_nodes_incremental 的 insert/update 路径复用。
    ///
    /// clip_count == 0：创建一个 QSGTextNode，整段 addTextLayout。
    /// clip_count > 0：按该段落的 visual lines 计算 complement，创建 clipNode + clipTextNode。
    /// vl_indices 是 vl_*_arr 中属于该段落的索引列表。
    ///
    /// 原子性：所有子节点先构建到 **detached staging 容器**，全部成功后才移入
    /// container。目标 QTextLayout 缺失或 createTextNode 失败时返回 false，
    /// container 保持原样（调用方保留旧节点树，不写 applied_*）。
    static bool build_paragraph_subtree(
        QSGNode* container, QQuickWindow* window,
        const char* text_ptr, int text_len,
        int cacheIdx, uint64_t epoch, double para_y,
        const int* vl_cache_idx_arr,
        const uint64_t* vl_epoch_arr,
        const int* vl_qtextline_idx_arr,
        const double* vl_y_arr, const double* vl_height_arr,
        const double* vl_doc_width_arr,
        const double* vl_para_y_arr,
        const bool* vl_has_qt_layout_arr,
        const std::vector<int>& vl_indices,
        const std::vector<std::pair<double,double>>& clip_rects_xy,
        const std::vector<std::pair<double,double>>& clip_rects_wh,
        float font_size, const QString& font_family,
        const QColor& textColor,
        double origin_x,
        uint64_t generation
    ) {
        (void)font_size;
        (void)font_family;
        if (!container || !window) return false;
        if (text_len == 0) return true; // 空段无需子节点，视为成功

        // Issue #904 评论 2: 用 get_paragraph_layout_checked 校验 (gen, slot, epoch)。
        // epoch=0 是合法第一版，一律走 checked 校验。
        QTextLayout* layout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
        if (!layout) return false; // 保留旧树，由调用方决定是否重试

        QSGNode* staging = new QSGNode;
        bool ok = true;

        if (clip_rects_xy.empty()) {
            // 无 clip：创建一个完整 QSGTextNode，整段 addTextLayout
            // 段内 y=0 构建，由 transform_node 平移到文档坐标。
            QSGTextNode* textNode = window->createTextNode();
            if (!textNode) {
                ok = false;
            } else {
                textNode->setColor(textColor);
                textNode->addTextLayout(QPointF(origin_x, 0.0), layout);
                textNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                staging->appendChildNode(textNode);
            }
        } else {
            // Issue #901 第十四轮问题3: 二维矩形差集精确裁剪。
            //
            // 对每个视觉行，用二维矩形差集（视觉行矩形减去所有与它相交的 clip 矩形）
            // 替代旧的 xClips + complement 方式。旧方式按"纵向有任何交集"就把 x 区间
            // 加入 xClips 对整行高度挖掉，导致部分 y 交集的 clip 挖掉过多。
            //
            // 二维差集算法：rect_subtract_holes(rect, holes) 返回 rect 减去所有 holes
            // 后的非重叠矩形集合。每个结果矩形创建一个 QSGClipNode + QSGTextNode。
            for (int vl_idx : vl_indices) {
                // Issue #901 第三十一轮问题1: 空段视觉行（has_qt_layout == false）
                // 跳过 layout 查询。build_paragraph_subtree 只在非空段调用，
                // 此处防御性检查保证不会误处理空段视觉行。
                if (!vl_has_qt_layout_arr[vl_idx]) continue;
                int qline = vl_qtextline_idx_arr[vl_idx];
                double ly = vl_y_arr[vl_idx];
                double lh = vl_height_arr[vl_idx];
                if (lh < 1.0) lh = 1.0;
                double dw = vl_doc_width_arr[vl_idx];
                // Issue #901 第十三轮问题3: paraY 不再用于 addTextLayout（段内 y=0），
                // 保留 paraY 仅供文档参考。clip rect 的 y 已改为段内坐标 (ly - para_y)。
                (void)vl_para_y_arr[vl_idx];

                double contentLeft = origin_x;
                double contentRight = dw - origin_x;
                if (contentRight < contentLeft) contentRight = contentLeft;

                // 视觉行矩形（文档坐标）
                double lineRect_x = contentLeft;
                double lineRect_y = ly;
                double lineRect_w = contentRight - contentLeft;
                double lineRect_h = lh;

                // 收集与该行矩形相交的 clip rects（二维交集）
                struct Rect { double x, y, w, h; };
                std::vector<Rect> holes;
                for (size_t c = 0; c < clip_rects_xy.size(); c++) {
                    double cx = clip_rects_xy[c].first;
                    double cy = clip_rects_xy[c].second;
                    double cw = clip_rects_wh[c].first;
                    double ch = clip_rects_wh[c].second;
                    // 二维交集判断：x 和 y 都有重叠
                    if (cx < lineRect_x + lineRect_w && cx + cw > lineRect_x &&
                        cy < lineRect_y + lineRect_h && cy + ch > lineRect_y) {
                        // clip 与行矩形相交，加入 holes
                        holes.push_back({cx, cy, cw, ch});
                    }
                }

                // 二维矩形差集：lineRect - holes
                // 算法：对每个 hole，从当前矩形集合中减去它。
                // rect - hole 最多产生 4 个子矩形（上、下、左、右）。
                std::vector<Rect> fragments;
                fragments.push_back({lineRect_x, lineRect_y, lineRect_w, lineRect_h});

                for (const auto& hole : holes) {
                    std::vector<Rect> next_fragments;
                    for (const auto& rect : fragments) {
                        // 检查 rect 和 hole 是否相交
                        double ix = std::max(rect.x, hole.x);
                        double iy = std::max(rect.y, hole.y);
                        double ix2 = std::min(rect.x + rect.w, hole.x + hole.w);
                        double iy2 = std::min(rect.y + rect.h, hole.y + hole.h);
                        if (ix >= ix2 || iy >= iy2) {
                            // 不相交，rect 保留
                            next_fragments.push_back(rect);
                            continue;
                        }
                        // 相交，计算 rect - hole 的子矩形
                        // 上条
                        if (iy > rect.y) {
                            next_fragments.push_back({rect.x, rect.y, rect.w, iy - rect.y});
                        }
                        // 下条
                        if (iy2 < rect.y + rect.h) {
                            next_fragments.push_back({rect.x, iy2, rect.w, rect.y + rect.h - iy2});
                        }
                        // 左条（y 范围与 hole 的 y 交集对齐）
                        if (ix > rect.x) {
                            next_fragments.push_back({rect.x, iy, ix - rect.x, iy2 - iy});
                        }
                        // 右条
                        if (ix2 < rect.x + rect.w) {
                            next_fragments.push_back({ix2, iy, rect.x + rect.w - ix2, iy2 - iy});
                        }
                    }
                    fragments = std::move(next_fragments);
                }

                // 为每个差集结果矩形创建 QSGClipNode + QSGTextNode
                for (const auto& frag : fragments) {
                    double clipW = frag.w;
                    double clipH = frag.h;
                    if (clipW < 0.5 || clipH < 0.5) continue;

                    auto* clipNode = new QSGClipNode;
                    clipNode->setIsRectangular(true);
                    // Issue #901 第十三轮问题3: clip rect 转为段内坐标（y - para_y），
                    // 由 transform_node 平移回文档坐标。
                    clipNode->setClipRect(QRectF(frag.x, frag.y - para_y, clipW, clipH));

                    QSGTextNode* clipTextNode = window->createTextNode();
                    if (clipTextNode) {
                        clipTextNode->setColor(textColor);
                        clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                        clipNode->appendChildNode(clipTextNode);
                        // 段内 y=0 构建。
                        clipTextNode->addTextLayout(QPointF(origin_x, 0.0), layout, -1, -1, qline, 1);
                        clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                        staging->appendChildNode(clipNode);
                    } else {
                        delete clipNode;
                        ok = false;
                    }
                }
            }
        }

        if (!ok) {
            // createTextNode 失败：不污染 container，保留旧树。
            delete staging;
            return false;
        }
        // 原子移动：staging 的子节点全部移入 container，再删除空壳。
        while (staging->childCount() > 0) {
            QSGNode* child = staging->childAtIndex(0);
            staging->removeChildNode(child);
            container->appendChildNode(child);
        }
        delete staging;
        return true;
    }

    /// Issue #904 评论 2: 增量更新静态正文 QSG 节点。
    ///
    /// 基于 ParagraphId 持久维护 per-item 的 ParagraphId→QSGNode 映射，
    /// 单次调用仅 insert/remove/update 变化的段落节点，不全量重建。
    ///
    /// - 新出现的 ParagraphId → insert 新容器节点 + 子节点
    /// - 消失的 ParagraphId → remove 容器节点（从 staticLayer 移除并删除）
    /// - 已存在且 force_full_update=true → update（删除旧子节点，创建新子节点）
    /// - 已存在且 force_full_update=false → 按 per-paragraph content_revision 决定：
    ///   - content_revision 相同且 para_y 相同 → 跳过
    ///   - content_revision 相同但 para_y 不同 → 只更新 para_y
    ///   - content_revision 不同 → 重建子节点
    ///
    /// Issue #901 第十四轮问题2: force_full_update 只用于真正全局变化
    /// （字体/宽度/字号变化），普通正文输入不应 force_full_update=true。
    /// force_full_update=false 时由 per-paragraph content_revision 独立判断。
    ///
    /// Issue #901 第十四轮问题4: g_incremental_para_map key 改为 staticLayer 指针，
    /// session_id 变化时清空 para_map。
    ///
    /// 返回 true 表示成功；false 表示因 layout 缺失或场景图无效而放弃
    /// （调用方应保留 dirty 标志，下一帧重试）。
    bool update_text_nodes_incremental(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
        const int* cache_idx_arr,
        const uint64_t* para_epoch_arr,
        const double* para_y_arr,
        const uint64_t* paragraph_id_arr,
        const uint64_t* content_revision_arr,
        const double* paragraph_height_arr,
        int para_count,
        // Issue #904 复核 1: 整体段落 y 偏移（复用已有 transform 插值）。
        const uint64_t* offset_id_arr,
        const double* offset_val_arr,
        int offset_count,
        const int* vl_cache_idx_arr,
        const uint64_t* vl_epoch_arr,
        const int* vl_qtextline_idx_arr,
        const double* vl_y_arr, const double* vl_height_arr,
        const double* vl_doc_width_arr,
        const double* vl_para_y_arr,
        const uint64_t* vl_paragraph_id_arr,
        const bool* vl_has_qt_layout_arr,
        int vl_count,
        float font_size, const QString& font_family,
        double scroll_y, const QString& color_q,
        const double* clip_x_arr, const double* clip_y_arr,
        const double* clip_w_arr, const double* clip_h_arr,
        int clip_count,
        double origin_x,
        uint64_t generation,
        bool force_full_update,
        uint64_t session_id
    ) {
        if (!root_raw || !item_ptr) return false;
        QQuickWindow* window = item_ptr->window();
        if (!window) return false;

        if (root_raw->childCount() == 0) return false;
        auto* staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return false;

        // 检查所有非空段落的 layout 可用性
        // Issue #901 第三十轮问题1: 用 get_paragraph_layout_checked 校验 epoch。
        // Issue #901 第三十一轮问题2: epoch=0 是合法第一版，一律走 checked 校验。
        for (int i = 0; i < para_count; i++) {
            if (text_lens[i] == 0) continue;
            int cacheIdx = cache_idx_arr[i];
            uint64_t epoch = para_epoch_arr[i];
            QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
            if (!cachedLayout) return false;
        }
        // 检查 visual line layout 可用性
        // Issue #901 第三十一轮问题1: 空段视觉行（has_qt_layout == false）跳过
        // layout 查询，不 return false。
        if (clip_count > 0 && vl_count > 0) {
            for (int i = 0; i < vl_count; i++) {
                if (!vl_has_qt_layout_arr[i]) continue;
                int cacheIdx = vl_cache_idx_arr[i];
                uint64_t epoch = vl_epoch_arr[i];
                QTextLayout* cachedLayout = get_paragraph_layout_checked(generation, cacheIdx, epoch);
                if (!cachedLayout) return false;
            }
        }

        QColor textColor(color_q);
        // Issue #901 第十四轮问题4: key 改为 staticLayer 指针。
        // staticLayer 是 QSG 树节点指针，item 销毁后 QSG 树被回收，
        // 新 item 生成新 staticLayer 指针，不会地址复用。
        auto& item_map = g_incremental_para_map[staticLayer];
        // 记录 item_ptr，供 cleanup 函数按 item 查找并清理 map 条目。
        item_map.item_ptr = item_ptr;
        // 安全检查：如果 staticLayer 变了（item 销毁后重建生成新 QSG 树），
        // 旧 map 条目中的 container 指针已悬空。清空 map（不 delete 节点，
        // 它们属于旧 QSG 树，已被 Qt 管理），重新开始。
        if (item_map.static_layer != staticLayer) {
            item_map.para_map.clear();
            item_map.static_layer = staticLayer;
        }
        // Issue #901 第十四轮问题4: session_id 变化时清空 para_map。
        // 切章/清文档时 session_id 递增，旧段落节点不再有效。
        if (item_map.session_id != session_id) {
            // 清空 staticLayer 的所有旧子节点（旧 session 的段落节点）
            while (staticLayer->childCount() > 0) {
                QSGNode* oldChild = staticLayer->childAtIndex(staticLayer->childCount() - 1);
                staticLayer->removeChildNode(oldChild);
                delete oldChild;
            }
            item_map.para_map.clear();
            item_map.session_id = session_id;
        }
        auto& node_map = item_map.para_map;

        // Issue #904 复核 1: 整体段落 y 偏移查找。换行/回流使下游段落下移时，
        // 复用已有 QSGTransformNode 从旧 y 插值到新 y，不逐字复制纹理。
        auto offset_for = [&](uint64_t pid) -> double {
            for (int k = 0; k < offset_count; k++) {
                if (offset_id_arr[k] == pid) return offset_val_arr[k];
            }
            return 0.0;
        };

        // Issue #901 第十一轮问题3: 增量第一次接管已有旧树。
        // node_map 为空但 staticLayer 已有子节点，说明之前走的是全量 rebuild
        // 路径（rebuild_text_node_from_paragraphs 已清空 map），旧子节点没有
        // 对应的增量映射。若直接 insert 新容器节点会 append 到旧子节点旁，
        // 造成双层文字。必须先清除 staticLayer 的所有旧子节点，再建立增量映射。
        // 此处删除旧子节点是安全的：它们由全量 rebuild 创建，无人通过 map 持有
        // 悬空引用（map 已为空）。
        if (node_map.empty() && staticLayer->childCount() > 0) {
            while (staticLayer->childCount() > 0) {
                QSGNode* oldChild = staticLayer->childAtIndex(staticLayer->childCount() - 1);
                staticLayer->removeChildNode(oldChild);
                delete oldChild;
            }
        }

        // 构建 needed_ids 集合
        std::unordered_set<uint64_t> needed_ids;
        for (int i = 0; i < para_count; i++) {
            needed_ids.insert(paragraph_id_arr[i]);
        }

        // ── Remove: 删除不在 needed_ids 中的已有节点 ──
        std::vector<uint64_t> to_remove;
        for (auto& kv : node_map) {
            if (needed_ids.find(kv.first) == needed_ids.end()) {
                to_remove.push_back(kv.first);
            }
        }
        for (uint64_t id : to_remove) {
            auto it = node_map.find(id);
            if (it != node_map.end()) {
                QSGTransformNode* transform_node = it->second.transform_node;
                staticLayer->removeChildNode(transform_node);
                delete transform_node;
                node_map.erase(it);
            }
        }

        // ── 准备全量 clipRects ──
        std::vector<std::pair<double,double>> all_clip_rects_xy;
        std::vector<std::pair<double,double>> all_clip_rects_wh;
        for (int c = 0; c < clip_count; c++) {
            double cw = clip_w_arr[c];
            double ch = clip_h_arr[c];
            if (ch > 0.0 && cw > 0.0) {
                all_clip_rects_xy.push_back({clip_x_arr[c], clip_y_arr[c]});
                all_clip_rects_wh.push_back({cw, ch});
            }
        }

        // ── 按 paragraph_id 分组 visual lines ──
        std::unordered_map<uint64_t, std::vector<int>> vl_for_para;
        for (int i = 0; i < vl_count; i++) {
            vl_for_para[vl_paragraph_id_arr[i]].push_back(i);
        }

        // ── Insert / Update ──
        // Issue #901 第十四轮问题2: 按 per-paragraph content_revision 和
        // per-paragraph filtered clip signature 驱动增量更新。
        // - force_full_update=true: 无条件重建所有已有段落子节点（真正全局变化）。
        // - force_full_update=false:
        //     - para_y 变化 → 只更新 transform_node 矩阵（不重建子节点）
        //     - content_revision 变化 → 重建子节点（内容变了）
        //     - filtered clip signature 变化 → 重建子节点（裁剪变了）
        //     - 都不变 → 真正跳过
        for (int i = 0; i < para_count; i++) {
            uint64_t id = paragraph_id_arr[i];
            double para_y = para_y_arr[i];
            int cacheIdx = cache_idx_arr[i];
            bool is_empty = (text_lens[i] == 0);
            uint64_t content_revision = content_revision_arr[i];
            double para_height = paragraph_height_arr[i];

            // 计算该段的 visual line qtextline 索引集合（clip signature 的一部分）
            auto vl_it = vl_for_para.find(id);
            std::vector<int> empty_vl;
            const std::vector<int>& vl_indices = (vl_it != vl_for_para.end()) ? vl_it->second : empty_vl;
            std::vector<int> para_vl_qtextline_idxs;
            para_vl_qtextline_idxs.reserve(vl_indices.size());
            for (int vl_idx : vl_indices) {
                para_vl_qtextline_idxs.push_back(vl_qtextline_idx_arr[vl_idx]);
            }

            // Issue #901 第十四轮问题2: 按段 y 范围过滤 clip。
            // 只保留与该段 y 范围 [para_y, para_y + para_height) 相交的 clip，
            // 而非整页 clip。这样无关 clip 变化不会触发该段重建。
            std::vector<std::pair<double,double>> para_clip_xy;
            std::vector<std::pair<double,double>> para_clip_wh;
            double para_top = para_y;
            double para_bottom = para_y + para_height;
            for (size_t c = 0; c < all_clip_rects_xy.size(); c++) {
                double cy = all_clip_rects_xy[c].second;
                double ch = all_clip_rects_wh[c].second;
                if (cy < para_bottom && cy + ch > para_top) {
                    para_clip_xy.push_back(all_clip_rects_xy[c]);
                    para_clip_wh.push_back(all_clip_rects_wh[c]);
                }
            }

            auto it = node_map.find(id);
            if (it != node_map.end()) {
                // 已有节点
                // 问题3: para_y 变化只更新 transform_node 矩阵，不重建子节点
                // Issue #904 复核 1: 整体段落 y 偏移也走同一矩阵插值。
                double y_offset = offset_for(id);
                if (it->second.para_y != para_y || it->second.applied_y_offset != y_offset) {
                    QMatrix4x4 para_mat;
                    para_mat.translate(0.0f, static_cast<float>(para_y + y_offset), 0.0f);
                    it->second.transform_node->setMatrix(para_mat);
                    it->second.transform_node->markDirty(QSGNode::DirtyMatrix);
                    it->second.para_y = para_y;
                    it->second.applied_y_offset = y_offset;
                }

                // Issue #901 第十四轮问题2: 用 per-paragraph content_revision 判断内容变化。
                // 不再用全局 generation，只比较该段自己的 content_revision。
                bool content_changed = (it->second.applied_content_revision != content_revision) ||
                                       (it->second.applied_cache_idx != cacheIdx) ||
                                       (it->second.applied_epoch != para_epoch_arr[i]);
                // Issue #901 第十四轮问题2: 用 per-paragraph filtered clip signature。
                bool clip_changed = (it->second.applied_clip_xy != para_clip_xy) ||
                                    (it->second.applied_clip_wh != para_clip_wh) ||
                                    (it->second.applied_vl_qtextline_idxs != para_vl_qtextline_idxs);
                bool need_rebuild = force_full_update || content_changed || clip_changed;

                if (!need_rebuild) {
                    continue;
                }

                // Issue #904 评论 2: 在 detached 新 transform_node 上先构建完整子树，
                // 成功后才用 insertChildNodeBefore + removeChildNode 原子替换旧节点。
                // 绝不先删旧子节点再尝试构建（那正是正文消失的根因）。
                QSGTransformNode* old_tn = it->second.transform_node;
                QSGTransformNode* new_tn = new QSGTransformNode;
                QMatrix4x4 new_mat;
                new_mat.translate(0.0f, static_cast<float>(para_y + y_offset), 0.0f);
                new_tn->setMatrix(new_mat);
                bool built = true;
                if (!is_empty) {
                    built = build_paragraph_subtree(
                        new_tn, window,
                        text_ptrs[i], text_lens[i],
                        cacheIdx, para_epoch_arr[i], para_y,
                        vl_cache_idx_arr, vl_epoch_arr, vl_qtextline_idx_arr,
                        vl_y_arr, vl_height_arr,
                        vl_doc_width_arr, vl_para_y_arr,
                        vl_has_qt_layout_arr,
                        vl_indices,
                        para_clip_xy, para_clip_wh,
                        font_size, font_family,
                        textColor, origin_x, generation
                    );
                }
                if (!built) {
                    // 目标 layout 缺失/构建失败：保留旧树与旧 applied_*，本帧放弃。
                    delete new_tn;
                    return false;
                }
                staticLayer->insertChildNodeBefore(new_tn, old_tn);
                staticLayer->removeChildNode(old_tn);
                delete old_tn;
                it->second.transform_node = new_tn;
                it->second.para_y = para_y;
                it->second.applied_y_offset = y_offset;
                it->second.applied_content_revision = content_revision;
                it->second.applied_cache_idx = cacheIdx;
                it->second.applied_epoch = para_epoch_arr[i];
                it->second.applied_clip_xy = para_clip_xy;
                it->second.applied_clip_wh = para_clip_wh;
                it->second.applied_vl_qtextline_idxs = para_vl_qtextline_idxs;
            } else {
                // 新节点：创建段落位置层 QSGTransformNode
                // Issue #904 复核 1: 初始矩阵带本帧整体 y 偏移。
                double new_y_offset = offset_for(id);
                QSGTransformNode* transform_node = new QSGTransformNode;
                QMatrix4x4 para_mat;
                para_mat.translate(0.0f, static_cast<float>(para_y + new_y_offset), 0.0f);
                transform_node->setMatrix(para_mat);
                bool built = true;
                if (!is_empty) {
                    built = build_paragraph_subtree(
                        transform_node, window,
                        text_ptrs[i], text_lens[i],
                        cacheIdx, para_epoch_arr[i], para_y,
                        vl_cache_idx_arr, vl_epoch_arr, vl_qtextline_idx_arr,
                        vl_y_arr, vl_height_arr,
                        vl_doc_width_arr, vl_para_y_arr,
                        vl_has_qt_layout_arr,
                        vl_indices,
                        para_clip_xy, para_clip_wh,
                        font_size, font_family,
                        textColor, origin_x, generation
                    );
                }
                if (!built) {
                    delete transform_node;
                    return false;
                }
                // Issue #904 评论 2: 直接插到文档顺序的正确位置（第一个 para_y 更大的节点前），
                // 不再事后 remove/append 整表重排。
                QSGNode* before = nullptr;
                for (auto& kv : node_map) {
                    if (kv.second.para_y > para_y) {
                        before = kv.second.transform_node;
                        break;
                    }
                }
                if (before) {
                    staticLayer->insertChildNodeBefore(transform_node, before);
                } else {
                    staticLayer->appendChildNode(transform_node);
                }
                IncrementalParaNodeInfo info;
                info.transform_node = transform_node;
                info.para_y = para_y;
                info.applied_y_offset = new_y_offset;
                info.applied_content_revision = content_revision;
                info.applied_cache_idx = cacheIdx;
                info.applied_epoch = para_epoch_arr[i];
                info.applied_clip_xy = para_clip_xy;
                info.applied_clip_wh = para_clip_wh;
                info.applied_vl_qtextline_idxs = para_vl_qtextline_idxs;
                node_map[id] = info;
            }
        }

        // Issue #904 评论 2: 子节点顺序在 insert 时已按 para_y 保持（新节点插到
        // 第一个 para_y 更大的节点前，已存在节点只改矩阵），不再每帧 remove/append
        // 整表重排。仅在更新/删除后也不影响既有相对顺序。

        // ── 更新滚动位移矩阵 ──
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);

        return true;
    }

    /// Issue #901 第十轮问题4+第十四轮问题4: 清理指定 item 的 ParagraphId→QSGNode 映射。
    ///
    /// 在渲染线程调用（如全量 rebuild 回退前），避免全局 map 中残留悬空指针。
    /// 不删除 QSGNode（由 Qt 场景图自行管理），只清理 map 条目。
    /// Issue #901 第十四轮问题4: map key 改为 QSGTransformNode*（staticLayer），
    /// 此函数按 item_ptr 遍历 map 查找并清理对应条目。
    void clear_incremental_para_map(QQuickItem* item_ptr) {
        if (!item_ptr) return;
        // 按 item_ptr 遍历 map，清理所有匹配条目（通常只有 1 个）。
        for (auto it = g_incremental_para_map.begin(); it != g_incremental_para_map.end(); ) {
            if (it->second.item_ptr == item_ptr) {
                it = g_incremental_para_map.erase(it);
            } else {
                ++it;
            }
        }
    }

    // Issue #901 第十三轮问题5+第十四轮问题4: 通过 QQuickWindow::scheduleRenderJob
    // 在渲染线程安全清理 g_incremental_para_map 中属于 item_ptr 的条目。
    //
    // Qt 官方文档：releaseResources() 在 GUI 线程触发，涉及 render 资源的清理
    // 需使用 QQuickWindow::scheduleRenderJob()，在渲染线程执行 job。
    // g_incremental_para_map 被 update_paint_node（渲染线程）读写，
    // GUI 线程直接 erase 是跨线程数据竞争。scheduleRenderJob 保证 job 在渲染线程执行，
    // 与同在渲染线程的 update_text_nodes_incremental 读写互斥安全。
    //
    // Issue #901 第十四轮问题4: cleanup 按 item_ptr 遍历 map 查找，
    // 不解引用 item_ptr，所以即使 job 执行时 item 已被销毁，item_ptr 仍可安全比较。
    class ClearIncrementalParaMapJob : public QRunnable {
    public:
        ClearIncrementalParaMapJob(void* key) : m_key(key) {}
        void run() override {
            // 按 item_ptr 遍历 map，清理所有匹配条目。
            for (auto it = g_incremental_para_map.begin(); it != g_incremental_para_map.end(); ) {
                if (it->second.item_ptr == m_key) {
                    it = g_incremental_para_map.erase(it);
                } else {
                    ++it;
                }
            }
        }
    private:
        void* m_key;
    };

    /// Issue #901 第十三轮问题5: 在 GUI 线程调用，通过 scheduleRenderJob
    /// 调度渲染线程清理 g_incremental_para_map 中属于 item_ptr 的条目。
    /// 用于 QQuickItem::releaseResources() 和切章/清文档场景。
    void schedule_clear_incremental_para_map(QQuickItem* item_ptr) {
        if (!item_ptr) return;
        QQuickWindow* window = item_ptr->window();
        if (!window) {
            // 没有 window 时无法调度 render job。
            // 此时渲染线程不会为该 item 调用 update_paint_node（item 已脱离 window），
            // 但其他 item 的渲染可能正在读写 g_incremental_para_map。
            // std::unordered_map 的 erase 不是线程安全的（即使不同 key），
            // 所以不能在 GUI 线程直接 erase。保留条目（按 item_ptr 索引，
            // 不影响其他 item），等 sceneGraphInvalidated 时统一清理。
            return;
        }
        // AfterSynchronizingStage: 在 GUI↔render 同步之后、下一帧渲染之前执行，
        // 此时渲染线程空闲，与 update_paint_node 互斥安全。
        window->scheduleRenderJob(
            new ClearIncrementalParaMapJob(item_ptr),
            QQuickWindow::AfterSynchronizingStage
        );
    }

    /// Issue #901 第十三轮问题5: 在渲染线程直接清理（用于 sceneGraphInvalidated 回调）。
    /// sceneGraphInvalidated 在渲染线程触发，回调直接在渲染线程执行，
    /// 与 update_paint_node 互斥安全。
    /// Issue #901 第十四轮问题4: 按 item_ptr 遍历 map 查找并清理。
    void clear_incremental_para_map_render_thread(void* item_ptr) {
        if (!item_ptr) return;
        for (auto it = g_incremental_para_map.begin(); it != g_incremental_para_map.end(); ) {
            if (it->second.item_ptr == item_ptr) {
                it = g_incremental_para_map.erase(it);
            } else {
                ++it;
            }
        }
    }
}}

/// 由 `EditorLayout::snapshot()` 提供的 per-paragraph 排版数据。
///
/// Issue #658: 改为按段落传入数据（不再按视觉行）。cache_idx 是
/// g_paragraph_layout_cache 中的索引，用于查找已排好的 QTextLayout。
/// 主 textNode 按段落整段 addTextLayout。
pub(crate) struct ParagraphLineInfo {
    /// 段落文本（不含尾部 \n）
    pub paragraph_text: String,
    /// 段落在文档中的字节偏移
    pub para_start: usize,
    /// Issue #901 第九轮阻塞4: 稳定段落身份标识（不随编辑移动）。
    /// 由 `ParagraphLayoutStore` 分配，跨编辑保持不变。
    /// C++ 侧据此定位节点，不依赖 byte offset 或子节点下标。
    pub paragraph_id: u64,
    /// 该段落在 g_paragraph_layout_cache 中的索引
    pub cache_idx: i32,
    /// Issue #901 第三十轮问题1: 该段落 layout 的 epoch。
    /// C++ 侧用 (generation, cache_idx, layout_epoch) 通过
    /// get_paragraph_layout_checked 校验 epoch 后查布局，
    /// 防止旧帧误取同槽新对象。
    /// Issue #901 第三十一轮问题2: 0 是合法的第一版 epoch，一律走 checked 校验。
    pub layout_epoch: u64,
    /// 段落第一行在文档中的 y 坐标（文档坐标，即 VisualLine.y）
    pub y: f64,
    /// 段落第一行的缩进宽度
    pub indent_w: f64,
    /// 字号
    pub font_size: f32,
    /// 字体族
    pub font_family: String,
    /// 段落换行宽度（用于 C++ complement 区间判断）
    pub doc_width: f64,
    /// Issue #901 第十四轮问题2: 段落内容版本签名（per-paragraph content revision）。
    ///
    /// 随段落文本/排版变化递增的计数器，而非全局 generation。
    /// C++ 侧据此判断该段内容是否变化，不受其他段落变化影响。
    /// 由 `hash_str(paragraph_text)` 计算，与 `ParagraphLayoutCacheKey::text_hash` 一致。
    pub content_revision: u64,
    /// Issue #901 第十四轮问题2: 段落总高度（所有视觉行高度之和）。
    ///
    /// C++ 侧据此过滤 clip：只保留与该段 y 范围 `[y, y + paragraph_height)` 相交的 clip，
    /// 而非保存整页 clip。这样某段无关 clip 变化不会触发该段重建。
    pub paragraph_height: f64,
}

/// Issue #658: per-visual-line 裁剪数据 — 用于按视觉行裁剪动画接管区域。
///
/// 每个视觉行携带自己的 cache_slot、qtextline_idx、y、height、x、width，
/// 以及所属段落的 para_y（addTextLayout 偏移）。裁剪时用 QSGTextNode::addTextLayout
/// 的 lineStart/lineCount 参数按单独视觉行绘制，setClipRect 用视觉行 y/h。
pub(crate) struct VisualLineClipInfo {
    /// 该视觉行所属段落在 g_paragraph_layout_cache 中的索引
    pub cache_idx: i32,
    /// Issue #901 第三十轮问题1: 该视觉行所属段落 layout 的 epoch。
    /// C++ 侧用 (generation, cache_idx, layout_epoch) 通过
    /// get_paragraph_layout_checked 校验 epoch 后查布局，
    /// 防止旧帧误取同槽新对象。
    /// Issue #901 第三十一轮问题2: 0 是合法的第一版 epoch，一律走 checked 校验。
    pub layout_epoch: u64,
    /// 该视觉行在 QTextLayout 中的行索引（lineStart 参数）
    pub qtextline_idx: i32,
    /// 该视觉行在文档中的 y 坐标
    pub y: f64,
    /// 该视觉行的行高
    pub height: f64,
    /// 该视觉行的 x 坐标（含缩进）
    pub x: f64,
    /// 该视觉行的宽度
    pub width: f64,
    /// 该视觉行所属段落第一行的 y 坐标（addTextLayout 偏移）
    pub para_y: f64,
    /// 文档宽度（complement 区间右边界）
    pub doc_width: f64,
    /// Issue #901 第十轮问题4: 该视觉行所属段落的稳定 ParagraphId。
    /// C++ 侧据此把 visual line 归组到正确的段落容器节点，
    /// 实现按 ParagraphId 的增量 insert/remove/update。
    pub paragraph_id: u64,
    /// Issue #901 第三十一轮问题1: 该视觉行所属段落是否有 QTextLayout。
    /// - true：非空段，C++ 渲染路径用 get_paragraph_layout_checked 查 layout。
    /// - false：空段（cache_idx < 0），C++ 渲染路径跳过 layout 查询
    ///   （不 return false），但其高度、段落范围和占位保留。
    pub has_qt_layout: bool,
}

/// 动画接管区域的裁剪矩形（文档坐标 x/y/w/h）。
///
/// Issue #901 问题3：这是静态层的**排除区**，覆盖动画画布接管的**完整目标范围**，
/// 整个过渡期间与 `progress` 无关。只在接管集合改变时重建静态 QSGTextNode，
/// 不在每帧改变挖空矩形。动画层的动态 clip 才按 `progress` 更新。
///
/// Issue #901 问题2：使用文档坐标，C++ 静态层（`QSGTransformNode`）统一施加
/// 一次 `translate(0, -scroll_y)` 变换。
///
/// Issue #658：改为完整的 x/y/w/h 文档坐标矩形，
/// 由 build_render_plan_full() 通过 PreparedLineSnapshot::source_rect_to_document_rect() 转换。
#[derive(Clone, Debug)]
pub(crate) struct AnimationClipRect {
    /// 文档 x 坐标
    pub x: f64,
    /// 文档 y 坐标
    pub y: f64,
    /// 宽度
    pub w: f64,
    /// 高度
    pub h: f64,
    /// Issue #736 评论 5786231506: 这个 clip 属于哪个 snapshot_id，
    /// 渲染前用于判断该 overlay 纹理是否真的存在。Rust 侧 ownership 元数据，
    /// 不交给 Qt 绘制接口本身（C++ 侧 rebuild_text_node_from_paragraphs 不消费此字段）。
    ///
    /// 纹理依赖型 exclusion 在决定静态 / 动画所有权前使用此 id 检查资源。
    pub snapshot_id: super::layout_snapshot::LineSnapshotId,
}

/// 静态层 exclusion 的同视觉行区间合并。
///
/// 调用方先确认完整动画资源，再合并同一目标纹理上的相邻区间。
///
/// 四条硬规则，任何一条破坏都会让静态正文出现空洞或误裁：
/// 1. 只在**同一视觉行**（`y` / `h` 相同）且**同一 `snapshot_id`** 的组内处理。
///    跨 `snapshot_id` 合并会破坏纹理缺失回退：snapshot A 纹理存在、B 缺失时，
///    合并并挂到 A 上会让 B 那块静态正文被裁掉，而 B 的动画 glyph 画不出来。
/// 2. 只合并**相交或相邻**的区间（`next_left <= current_right + EPS`）。
///    有 gap 就另起一条，否则中间的正常正文会被整段挖掉。
/// 3. **每一组**都执行 sweep，不只处理第一条。
pub(crate) fn merge_static_clip_rects(rects: Vec<AnimationClipRect>) -> Vec<AnimationClipRect> {
    const EPS: f64 = 1e-6;
    let mut kept: Vec<AnimationClipRect> = rects
        .into_iter()
        .filter(|r| r.w > 0.0 && r.h > 0.0)
        .collect();
    if kept.len() <= 1 {
        return kept;
    }
    // 排序只用 (y, h, x) 让同一视觉行的区间连续，然后扫连续段分组。
    kept.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.h.partial_cmp(&b.h).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut merged: Vec<AnimationClipRect> = Vec::with_capacity(kept.len());
    let mut slot: Option<AnimationClipRect> = None;
    for rect in kept {
        let Some(cur) = slot.as_ref() else {
            slot = Some(rect);
            continue;
        };
        // 换组（snapshot_id / 视觉行任一不同）：先落盘，另起一条。
        if cur.snapshot_id != rect.snapshot_id || cur.y != rect.y || cur.h != rect.h {
            if let Some(done) = slot.take() {
                merged.push(done);
            }
            slot = Some(rect);
            continue;
        }
        let cur_right = cur.x + cur.w;
        if rect.x <= cur_right + EPS {
            let left = cur.x.min(rect.x);
            let right = cur_right.max(rect.x + rect.w);
            slot = Some(AnimationClipRect {
                x: left,
                y: cur.y,
                w: right - left,
                h: cur.h,
                snapshot_id: cur.snapshot_id,
            });
        } else {
            if let Some(done) = slot.take() {
                merged.push(done);
            }
            slot = Some(rect);
        }
    }
    if let Some(done) = slot.take() {
        merged.push(done);
    }
    merged
}

/// Issue #658: 从已排好的 per-paragraph 数据重建静态正文 QSGTextNode。
///
/// 通过 cache_idx 在 g_paragraph_layout_cache 中查找已排好的 QTextLayout。
/// animation_clip_rects 使用 x/y/w/h 文档坐标。
/// visual_line_clips 提供 per-visual-line 裁剪数据，用于按视觉行裁剪。
/// `origin_x` 是正文左侧 padding，正文从 (origin_x, y) 开始绘制，
/// complement 裁剪横向范围按 [origin_x, doc_width - origin_x] 计算。
/// `generation` 标识本 snapshot 对应的 g_layout_generations 中的代，
/// 渲染时用 (generation, cache_slot) 查找 layout。
///
/// Issue #668 评论 5646458592 问题 1: 返回 bool 表示重建是否成功。
/// - true: 所有非空段落的 layout 都取到了，新 QSGTextNode 已替换进 staticLayer。
/// - false: 某个必需 layout 缺失，旧静态正文节点保持原样，调用方应保留
///   dirty 标志，下一帧继续处理正确的 snapshot/generation。
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针。
pub fn rebuild_text_node_from_paragraphs(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    paragraphs: &[ParagraphLineInfo],
    visual_line_clips: &[VisualLineClipInfo],
    scroll_y: f64,
    color: &str,
    animation_clip_rects: &[AnimationClipRect],
    origin_x: f64,
    generation: u64,
) -> bool {
    // Issue #668 评论 5646842592 问题 2: 空正文也必须走一次合法的静态层替换，
    // 把上一帧的旧正文节点从 staticLayer 上清掉。不能把 paragraphs.is_empty()
    // 当成"不需要更新"提前返回——那样 staticLayer 完全没动，上一帧旧正文继续
    // 留在屏幕上。C++ 侧 para_count=0 时不创建任何新 text/clip 节点（循环不
    // 执行），但仍执行"删除 staticLayer 全部旧 children"的原子替换（与问题 1
    // 修复一致），这样空正文会清掉旧正文。
    // 空 paragraphs 时 paragraphs[0] 会越界，此处提供合理默认 font_size/font_family
    // 传给 C++（与 SujianEditorItem::default 的默认值一致）。
    let font_size = paragraphs.first().map(|p| p.font_size).unwrap_or(16.0);
    let font_family_owned: String = paragraphs
        .first()
        .map(|p| p.font_family.clone())
        .unwrap_or_else(|| "Noto Sans CJK SC".to_string());
    let font_family = &font_family_owned;

    let font_family_q: qmetaobject::QString = font_family.clone().into();
    let color_q: qmetaobject::QString = color.to_string().into();

    let text_ptrs: Vec<*const u8> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.as_ptr())
        .collect();
    let text_lens: Vec<i32> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.len() as i32)
        .collect();
    let para_starts: Vec<i32> = paragraphs.iter().map(|p| p.para_start as i32).collect();
    let cache_idxs: Vec<i32> = paragraphs.iter().map(|p| p.cache_idx).collect();
    // Issue #901 第三十轮问题1: 构造 epoch 数组，传给 C++ 校验 layout。
    let para_epochs: Vec<u64> = paragraphs.iter().map(|p| p.layout_epoch).collect();
    let para_y: Vec<f64> = paragraphs.iter().map(|p| p.y).collect();
    let indent_ws: Vec<f64> = paragraphs.iter().map(|p| p.indent_w).collect();
    let doc_widths: Vec<f64> = paragraphs.iter().map(|p| p.doc_width).collect();

    let vl_cache_idxs: Vec<i32> = visual_line_clips.iter().map(|v| v.cache_idx).collect();
    // Issue #901 第三十轮问题1: 构造 visual line epoch 数组。
    let vl_epochs: Vec<u64> = visual_line_clips.iter().map(|v| v.layout_epoch).collect();
    let vl_qtextline_idxs: Vec<i32> = visual_line_clips.iter().map(|v| v.qtextline_idx).collect();
    let vl_y: Vec<f64> = visual_line_clips.iter().map(|v| v.y).collect();
    let vl_height: Vec<f64> = visual_line_clips.iter().map(|v| v.height).collect();
    let vl_x: Vec<f64> = visual_line_clips.iter().map(|v| v.x).collect();
    let vl_width: Vec<f64> = visual_line_clips.iter().map(|v| v.width).collect();
    let vl_para_y: Vec<f64> = visual_line_clips.iter().map(|v| v.para_y).collect();
    let vl_doc_width: Vec<f64> = visual_line_clips.iter().map(|v| v.doc_width).collect();
    // Issue #901 第三十一轮问题1: 构造 visual line has_qt_layout 数组，
    // 传给 C++ 侧跳过空段视觉行的 layout 查询。
    let vl_has_qt_layout: Vec<bool> = visual_line_clips.iter().map(|v| v.has_qt_layout).collect();

    let clip_x: Vec<f64> = animation_clip_rects.iter().map(|c| c.x).collect();
    let clip_y: Vec<f64> = animation_clip_rects.iter().map(|c| c.y).collect();
    let clip_w: Vec<f64> = animation_clip_rects.iter().map(|c| c.w).collect();
    let clip_h: Vec<f64> = animation_clip_rects.iter().map(|c| c.h).collect();

    let para_count = paragraphs.len() as i32;
    let vl_count = visual_line_clips.len() as i32;
    let clip_count = animation_clip_rects.len() as i32;

    let text_ptrs_ptr = text_ptrs.as_ptr();
    let text_lens_ptr = text_lens.as_ptr();
    let para_starts_ptr = para_starts.as_ptr();
    let cache_idxs_ptr = cache_idxs.as_ptr();
    let para_epochs_ptr = para_epochs.as_ptr();
    let para_y_ptr = para_y.as_ptr();
    let indent_ws_ptr = indent_ws.as_ptr();
    let doc_widths_ptr = doc_widths.as_ptr();
    let vl_cache_idxs_ptr = vl_cache_idxs.as_ptr();
    let vl_epochs_ptr = vl_epochs.as_ptr();
    let vl_qtextline_idxs_ptr = vl_qtextline_idxs.as_ptr();
    let vl_y_ptr = vl_y.as_ptr();
    let vl_height_ptr = vl_height.as_ptr();
    let vl_x_ptr = vl_x.as_ptr();
    let vl_width_ptr = vl_width.as_ptr();
    let vl_para_y_ptr = vl_para_y.as_ptr();
    let vl_doc_width_ptr = vl_doc_width.as_ptr();
    let vl_has_qt_layout_ptr = vl_has_qt_layout.as_ptr();
    let clip_x_ptr = clip_x.as_ptr();
    let clip_y_ptr = clip_y.as_ptr();
    let clip_w_ptr = clip_w.as_ptr();
    let clip_h_ptr = clip_h.as_ptr();

    let success = cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptrs_ptr as "const char**",
        text_lens_ptr as "const int*",
        para_starts_ptr as "const int*",
        cache_idxs_ptr as "const int*",
        para_epochs_ptr as "const uint64_t*",
        para_y_ptr as "const double*",
        indent_ws_ptr as "const double*",
        doc_widths_ptr as "const double*",
        para_count as "int",
        vl_cache_idxs_ptr as "const int*",
        vl_epochs_ptr as "const uint64_t*",
        vl_qtextline_idxs_ptr as "const int*",
        vl_y_ptr as "const double*",
        vl_height_ptr as "const double*",
        vl_x_ptr as "const double*",
        vl_width_ptr as "const double*",
        vl_para_y_ptr as "const double*",
        vl_doc_width_ptr as "const double*",
        vl_has_qt_layout_ptr as "const bool*",
        vl_count as "int",
        font_size as "float",
        font_family_q as "QString",
        scroll_y as "double",
        color_q as "QString",
        clip_x_ptr as "const double*",
        clip_y_ptr as "const double*",
        clip_w_ptr as "const double*",
        clip_h_ptr as "const double*",
        clip_count as "int",
        origin_x as "double",
        generation as "uint64_t"
    ] -> bool as "bool" {
        return rebuild_text_node_from_paragraphs(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            para_starts_ptr, cache_idxs_ptr,
            para_epochs_ptr,
            para_y_ptr, indent_ws_ptr,
            doc_widths_ptr,
            para_count,
            vl_cache_idxs_ptr,
            vl_epochs_ptr,
            vl_qtextline_idxs_ptr,
            vl_y_ptr,
            vl_height_ptr,
            vl_x_ptr,
            vl_width_ptr,
            vl_para_y_ptr,
            vl_doc_width_ptr,
            vl_has_qt_layout_ptr,
            vl_count,
            font_size, font_family_q,
            scroll_y, color_q,
            clip_x_ptr, clip_y_ptr,
            clip_w_ptr, clip_h_ptr,
            clip_count,
            origin_x,
            generation
        );
    });

    // Issue #658: 不在 rebuild 末尾清除 g_paragraph_layout_cache。
    // 布局缓存由 editor_layout_lines() 填充，持久存在供后续帧读取；
    // 仅在 prepare_document_visual_snapshot() 开头和 editor_layout_lines() 开头清除。
    // Issue #668: 即使 rebuild 返回 false（layout 缺失），也不在此清除缓存——
    // 缺失说明 generation 已被提前 clear_layout_generation 释放，下一帧
    // snapshot() 会分配新 generation 重新排版。
    success
}

/// Issue #901 第五轮阻塞5: 只重建受影响段落的 QSGTextNode，不全文重建。
///
/// 与 `rebuild_text_node_from_paragraphs` 的区别：不删除和重建所有段落的节点，
/// 只对 `affected_indices` 中列出的段落删除旧 QSGTextNode 并创建新的。
/// 不在列表中的段落保持原节点不变，避免全文重建的开销。
///
/// **前提**：staticLayer 的子节点数必须等于 `paragraphs.len()`，且子节点按
/// 段落顺序排列。如果不匹配（如首次渲染或段落数变化），返回 `false`，
/// 调用方应回退到 `rebuild_text_node_from_paragraphs` 全量重建。
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针。
pub fn update_affected_text_nodes(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    paragraphs: &[ParagraphLineInfo],
    affected_indices: &[usize],
    scroll_y: f64,
    color: &str,
    origin_x: f64,
    generation: u64,
) -> bool {
    // Issue #901 第五轮阻塞2: 防御性空指针检查 — 如果 Qt 场景图根节点或
    // item 指针为空，说明场景图尚未初始化（如首次渲染前或组件销毁后），
    // 直接返回 false 让调用方回退到全量重建路径，避免 C++ 侧解引用空指针。
    if root_raw.is_null() || item_ptr.is_null() {
        return false;
    }

    if affected_indices.is_empty() {
        // 没有受影响段落，只更新滚动位移
        update_scroll_transform(root_raw, scroll_y);
        return true;
    }

    let font_size = paragraphs.first().map(|p| p.font_size).unwrap_or(16.0);
    let font_family_owned: String = paragraphs
        .first()
        .map(|p| p.font_family.clone())
        .unwrap_or_else(|| "Noto Sans CJK SC".to_string());
    let font_family = &font_family_owned;

    let font_family_q: qmetaobject::QString = font_family.clone().into();
    let color_q: qmetaobject::QString = color.to_string().into();

    let text_ptrs: Vec<*const u8> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.as_ptr())
        .collect();
    let text_lens: Vec<i32> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.len() as i32)
        .collect();
    let cache_idxs: Vec<i32> = paragraphs.iter().map(|p| p.cache_idx).collect();
    // Issue #901 第三十轮问题1: 构造 epoch 数组，传给 C++ 校验 layout。
    let para_epochs: Vec<u64> = paragraphs.iter().map(|p| p.layout_epoch).collect();
    let para_y: Vec<f64> = paragraphs.iter().map(|p| p.y).collect();

    let affected_idxs: Vec<i32> = affected_indices.iter().map(|&i| i as i32).collect();

    let para_count = paragraphs.len() as i32;
    let affected_count = affected_idxs.len() as i32;

    let text_ptrs_ptr = text_ptrs.as_ptr();
    let text_lens_ptr = text_lens.as_ptr();
    let cache_idxs_ptr = cache_idxs.as_ptr();
    let para_epochs_ptr = para_epochs.as_ptr();
    let para_y_ptr = para_y.as_ptr();
    let affected_idxs_ptr = affected_idxs.as_ptr();

    let success = cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptrs_ptr as "const char**",
        text_lens_ptr as "const int*",
        cache_idxs_ptr as "const int*",
        para_epochs_ptr as "const uint64_t*",
        para_y_ptr as "const double*",
        para_count as "int",
        affected_idxs_ptr as "const int*",
        affected_count as "int",
        font_size as "float",
        font_family_q as "QString",
        scroll_y as "double",
        color_q as "QString",
        origin_x as "double",
        generation as "uint64_t"
    ] -> bool as "bool" {
        return update_affected_paragraph_nodes(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            cache_idxs_ptr,
            para_epochs_ptr,
            para_y_ptr,
            para_count,
            affected_idxs_ptr, affected_count,
            font_size, font_family_q,
            scroll_y, color_q,
            origin_x,
            generation
        );
    });

    success
}

/// 滚动帧：只更新静态正文层的 QSGTransformNode 位移矩阵。
///
/// 不出现 clear()、QTextLayout、createLine()、addTextLayout()。
/// 在 `updatePaintNode()` 中 needs_relayout=false 时调用。
///
/// # Safety
/// `root_raw` 必须是有效的 Qt 场景图指针。
pub fn update_scroll_transform(root_raw: *mut std::ffi::c_void, scroll_y: f64) {
    if root_raw.is_null() {
        return;
    }
    cpp!(unsafe [
        root_raw as "QSGNode*",
        scroll_y as "double"
    ] {
        update_scroll_transform(root_raw, scroll_y);
    });
}

/// Issue #901 第十轮问题4: 增量更新静态正文 QSG 节点。
///
/// 基于 ParagraphId 持久维护 per-item 的 ParagraphId→QSGNode 映射，
/// 单次调用仅 insert/remove/update 变化的段落节点，不全量重建。
///
/// # 增量策略
///
/// - **新出现的 ParagraphId** → insert 新容器节点 + 子节点
/// - **消失的 ParagraphId** → remove 容器节点
/// - **已存在且 `force_full_update=true`** → update（删除旧子节点，创建新子节点）
/// - **已存在且 `force_full_update=false`** → 保持不动
///
/// `force_full_update=true` 用于 `needs_relayout` / clip 模式切换
/// 等需要重建所有已有段落子节点的场景。
/// `force_full_update=false` 用于纯滚动跨出（`viewport_exceeds_loaded`），
/// 只 insert 新进入视口的段落、remove 离开视口的段落，已有段落保持不动。
///
/// # 安全回退
///
/// 当 `paragraphs` 中存在 `paragraph_id == 0`（`ParagraphId::INVALID`）的段落时，
/// 增量映射无法正确区分不同段落（多个 id=0 会冲突）。此时返回 `false`，
/// 调用方应回退到 `rebuild_text_node_from_paragraphs` 全量重建。
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针。
pub fn update_text_nodes_incremental(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    paragraphs: &[ParagraphLineInfo],
    visual_line_clips: &[VisualLineClipInfo],
    scroll_y: f64,
    color: &str,
    animation_clip_rects: &[AnimationClipRect],
    paragraph_y_offsets: &[(u64, f64)],
    origin_x: f64,
    generation: u64,
    force_full_update: bool,
    session_id: u64,
) -> bool {
    // 防御性空指针检查
    if root_raw.is_null() || item_ptr.is_null() {
        return false;
    }

    // Issue #901 第十轮问题4: 检查是否存在 paragraph_id == 0 的段落。
    // ParagraphId::INVALID (0) 表示未分配有效 ID，多个段落共用 id=0 会在
    // C++ map 中冲突。此时回退到全量重建（调用方处理）。
    if paragraphs.iter().any(|p| p.paragraph_id == 0) {
        return false;
    }

    let font_size = paragraphs.first().map(|p| p.font_size).unwrap_or(16.0);
    let font_family_owned: String = paragraphs
        .first()
        .map(|p| p.font_family.clone())
        .unwrap_or_else(|| "Noto Sans CJK SC".to_string());
    let font_family = &font_family_owned;

    let font_family_q: qmetaobject::QString = font_family.clone().into();
    let color_q: qmetaobject::QString = color.to_string().into();

    let text_ptrs: Vec<*const u8> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.as_ptr())
        .collect();
    let text_lens: Vec<i32> = paragraphs
        .iter()
        .map(|p| p.paragraph_text.len() as i32)
        .collect();
    let cache_idxs: Vec<i32> = paragraphs.iter().map(|p| p.cache_idx).collect();
    // Issue #901 第三十轮问题1: 构造 epoch 数组，传给 C++ 校验 layout。
    let para_epochs: Vec<u64> = paragraphs.iter().map(|p| p.layout_epoch).collect();
    let para_y: Vec<f64> = paragraphs.iter().map(|p| p.y).collect();
    let paragraph_ids: Vec<u64> = paragraphs.iter().map(|p| p.paragraph_id).collect();
    let content_revisions: Vec<u64> = paragraphs.iter().map(|p| p.content_revision).collect();
    let paragraph_heights: Vec<f64> = paragraphs.iter().map(|p| p.paragraph_height).collect();

    let vl_cache_idxs: Vec<i32> = visual_line_clips.iter().map(|v| v.cache_idx).collect();
    // Issue #901 第三十轮问题1: 构造 visual line epoch 数组。
    let vl_epochs: Vec<u64> = visual_line_clips.iter().map(|v| v.layout_epoch).collect();
    let vl_qtextline_idxs: Vec<i32> = visual_line_clips.iter().map(|v| v.qtextline_idx).collect();
    let vl_y: Vec<f64> = visual_line_clips.iter().map(|v| v.y).collect();
    let vl_height: Vec<f64> = visual_line_clips.iter().map(|v| v.height).collect();
    let vl_doc_width: Vec<f64> = visual_line_clips.iter().map(|v| v.doc_width).collect();
    let vl_para_y: Vec<f64> = visual_line_clips.iter().map(|v| v.para_y).collect();
    let vl_paragraph_ids: Vec<u64> = visual_line_clips.iter().map(|v| v.paragraph_id).collect();
    // Issue #901 第三十一轮问题1: 构造 visual line has_qt_layout 数组，
    // 传给 C++ 侧跳过空段视觉行的 layout 查询。
    let vl_has_qt_layout: Vec<bool> = visual_line_clips.iter().map(|v| v.has_qt_layout).collect();

    let clip_x: Vec<f64> = animation_clip_rects.iter().map(|c| c.x).collect();
    let clip_y: Vec<f64> = animation_clip_rects.iter().map(|c| c.y).collect();
    let clip_w: Vec<f64> = animation_clip_rects.iter().map(|c| c.w).collect();
    let clip_h: Vec<f64> = animation_clip_rects.iter().map(|c| c.h).collect();

    // Issue #904 复核 1: 整体段落 y 偏移数组（复用已有 transform 插值）。
    let offset_ids: Vec<u64> = paragraph_y_offsets.iter().map(|(id, _)| *id).collect();
    let offset_values: Vec<f64> = paragraph_y_offsets
        .iter()
        .map(|(_, value)| *value)
        .collect();

    let para_count = paragraphs.len() as i32;
    let vl_count = visual_line_clips.len() as i32;
    let clip_count = animation_clip_rects.len() as i32;
    let offset_count = paragraph_y_offsets.len() as i32;

    let text_ptrs_ptr = text_ptrs.as_ptr();
    let text_lens_ptr = text_lens.as_ptr();
    let cache_idxs_ptr = cache_idxs.as_ptr();
    let para_epochs_ptr = para_epochs.as_ptr();
    let para_y_ptr = para_y.as_ptr();
    let paragraph_ids_ptr = paragraph_ids.as_ptr();
    let content_revisions_ptr = content_revisions.as_ptr();
    let paragraph_heights_ptr = paragraph_heights.as_ptr();
    let vl_cache_idxs_ptr = vl_cache_idxs.as_ptr();
    let vl_epochs_ptr = vl_epochs.as_ptr();
    let vl_qtextline_idxs_ptr = vl_qtextline_idxs.as_ptr();
    let vl_y_ptr = vl_y.as_ptr();
    let vl_height_ptr = vl_height.as_ptr();
    let vl_doc_width_ptr = vl_doc_width.as_ptr();
    let vl_para_y_ptr = vl_para_y.as_ptr();
    let vl_paragraph_ids_ptr = vl_paragraph_ids.as_ptr();
    let vl_has_qt_layout_ptr = vl_has_qt_layout.as_ptr();
    let clip_x_ptr = clip_x.as_ptr();
    let clip_y_ptr = clip_y.as_ptr();
    let clip_w_ptr = clip_w.as_ptr();
    let clip_h_ptr = clip_h.as_ptr();
    let offset_id_ptr = offset_ids.as_ptr();
    let offset_val_ptr = offset_values.as_ptr();

    let success = cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptrs_ptr as "const char**",
        text_lens_ptr as "const int*",
        cache_idxs_ptr as "const int*",
        para_epochs_ptr as "const uint64_t*",
        para_y_ptr as "const double*",
        paragraph_ids_ptr as "const uint64_t*",
        content_revisions_ptr as "const uint64_t*",
        paragraph_heights_ptr as "const double*",
        para_count as "int",
        offset_id_ptr as "const uint64_t*",
        offset_val_ptr as "const double*",
        offset_count as "int",
        vl_cache_idxs_ptr as "const int*",
        vl_epochs_ptr as "const uint64_t*",
        vl_qtextline_idxs_ptr as "const int*",
        vl_y_ptr as "const double*",
        vl_height_ptr as "const double*",
        vl_doc_width_ptr as "const double*",
        vl_para_y_ptr as "const double*",
        vl_paragraph_ids_ptr as "const uint64_t*",
        vl_has_qt_layout_ptr as "const bool*",
        vl_count as "int",
        font_size as "float",
        font_family_q as "QString",
        scroll_y as "double",
        color_q as "QString",
        clip_x_ptr as "const double*",
        clip_y_ptr as "const double*",
        clip_w_ptr as "const double*",
        clip_h_ptr as "const double*",
        clip_count as "int",
        origin_x as "double",
        generation as "uint64_t",
        force_full_update as "bool",
        session_id as "uint64_t"
    ] -> bool as "bool" {
        return update_text_nodes_incremental(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            cache_idxs_ptr,
            para_epochs_ptr,
            para_y_ptr,
            paragraph_ids_ptr,
            content_revisions_ptr,
            paragraph_heights_ptr,
            para_count,
            offset_id_ptr, offset_val_ptr, offset_count,
            vl_cache_idxs_ptr,
            vl_epochs_ptr,
            vl_qtextline_idxs_ptr,
            vl_y_ptr, vl_height_ptr,
            vl_doc_width_ptr,
            vl_para_y_ptr,
            vl_paragraph_ids_ptr,
            vl_has_qt_layout_ptr,
            vl_count,
            font_size, font_family_q,
            scroll_y, color_q,
            clip_x_ptr, clip_y_ptr,
            clip_w_ptr, clip_h_ptr,
            clip_count,
            origin_x,
            generation,
            force_full_update,
            session_id
        );
    });

    success
}

/// Issue #901 第十轮问题4: 清理指定 item 的 ParagraphId→QSGNode 映射。
///
/// 在渲染线程调用（如 sceneGraphInvalidated 回调），避免全局 map 中残留悬空指针。
/// 不删除 QSGNode（由 Qt 场景图自行管理），只清理 map 条目。
///
/// # Safety
/// `item_ptr` 必须是有效的 QQuickItem 指针或 null（null 时无操作）。
pub fn clear_incremental_para_map(item_ptr: *mut std::ffi::c_void) {
    if item_ptr.is_null() {
        return;
    }
    cpp!(unsafe [
        item_ptr as "QQuickItem*"
    ] {
        clear_incremental_para_map(item_ptr);
    });
}

/// Issue #901 第十三轮问题5: 在 GUI 线程调用，通过 QQuickWindow::scheduleRenderJob
/// 调度渲染线程清理 `g_incremental_para_map[item_ptr]`。
///
/// 用于 `QQuickItem::release_resources()` 和切章/清文档场景。
/// GUI 线程不能直接 erase 全局 map（与渲染线程的 `update_paint_node` 数据竞争），
/// `scheduleRenderJob` 保证 job 在渲染线程执行，与同在渲染线程的
/// `update_text_nodes_incremental` 读写互斥安全。
///
/// # Safety
/// `item_ptr` 必须是有效的 QQuickItem 指针或 null（null 时无操作）。
pub fn schedule_clear_incremental_para_map(item_ptr: *mut std::ffi::c_void) {
    if item_ptr.is_null() {
        return;
    }
    cpp!(unsafe [
        item_ptr as "QQuickItem*"
    ] {
        schedule_clear_incremental_para_map(item_ptr);
    });
}

/// Issue #901 第十三轮问题5: 在渲染线程直接清理（用于 sceneGraphInvalidated 回调）。
///
/// `sceneGraphInvalidated` 在渲染线程触发，回调直接在渲染线程执行，
/// 与 `update_paint_node` 互斥安全。
///
/// # Safety
/// `item_ptr` 是任意指针值（用作 map key，不解引用），可为 null（null 时无操作）。
pub fn clear_incremental_para_map_render_thread(item_ptr: *mut std::ffi::c_void) {
    if item_ptr.is_null() {
        return;
    }
    cpp!(unsafe [
        item_ptr as "void*"
    ] {
        clear_incremental_para_map_render_thread(item_ptr);
    });
}
