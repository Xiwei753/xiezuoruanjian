// SujianAnimationController — Windows 原生编辑动画控制器
//
// Issue #735：Core 不再提供 EditorVisualTransaction / AnimationMode / GlyphRect 等视觉类型。
// Windows 从 EditorEditResult（cause / operationKind）+ 自己的 DirectWrite text layout
// 推导动画策略并生成 ghost glyph 几何。
//
// 本文件只定义 Windows 自己的动画几何类型，不从 Core 获取。

using System;
using System.Collections.Generic;

namespace Sujian.Windows.Editor.Animation;

/// <summary>
/// Windows 原生 glyph 几何 — 由 DirectWrite text layout 计算得出。
/// 不从 Core 获取，Core 只提供编辑事实（cause / operationKind / offsetMap）。
/// </summary>
public sealed class WindowsGlyphRect
{
    public string Char { get; set; } = "";
    public float X { get; set; }
    public float Y { get; set; }
    public float Width { get; set; }
    public float Height { get; set; }
    public float BaselineY { get; set; }
}

/// <summary>
/// Windows 原生 reflow glyph 几何 — 表示一个字符从旧位置移动到新位置。
/// </summary>
public sealed class WindowsReflowGlyphRect
{
    public string Char { get; set; } = "";
    public float OldX { get; set; }
    public float OldY { get; set; }
    public float OldBaselineY { get; set; }
    public float NewX { get; set; }
    public float NewY { get; set; }
    public float NewBaselineY { get; set; }
    public float Width { get; set; }
    public float Height { get; set; }
}

/// <summary>
/// Windows 原生光标几何 — 由 DirectWrite caret position 计算得出。
/// </summary>
public sealed class WindowsCursorRect
{
    public float X { get; set; }
    public float Top { get; set; }
    public float Bottom { get; set; }
    public float BaselineY { get; set; }
}

/// <summary>
/// 动画语义类别 — 由 EditorEditResult.operationKind 推导。
/// 不再使用 Core 的 AnimationMode。
/// </summary>
public enum WindowsAnimationKind
{
    Insert,
    Delete,
    Cursor
}

/// <summary>
/// Windows 原生编辑动画输入 — 由 SujianEditorHost 从 EditorEditResult + DirectWrite layout 构造。
/// </summary>
public sealed class WindowsAnimationInput
{
    public ulong TransactionId { get; set; }
    public WindowsAnimationKind Kind { get; set; }
    public ulong DurationMs { get; set; }
    public WindowsCursorRect? OldCursorRect { get; set; }
    public WindowsCursorRect? NewCursorRect { get; set; }
    public List<WindowsGlyphRect> InsertGlyphRects { get; set; } = new();
    public List<WindowsGlyphRect> DeletedGlyphRects { get; set; } = new();
    public List<WindowsReflowGlyphRect> ReflowGlyphRects { get; set; } = new();
}

public enum GhostAnimKind
{
    Insert,
    Delete,
    Reflow
}

public sealed class ActiveAnimation
{
    public ulong TransactionId { get; set; }
    public WindowsAnimationKind Kind { get; set; }
    public DateTime StartTime { get; set; }
    public ulong DurationMs { get; set; }
    public List<GhostGlyph> Ghosts { get; } = new();
    public bool IsFinished { get; set; }
}

public sealed class GhostGlyph
{
    public string Char { get; set; } = "";
    public float OriginStartX { get; set; }
    public float OriginStartY { get; set; }
    public float EndX { get; set; }
    public float EndY { get; set; }
    public float Width { get; set; }
    public float Height { get; set; }
    public float BaselineY { get; set; }
    public GhostAnimKind AnimKind { get; set; } = GhostAnimKind.Insert;
    public float CurrentX { get; set; }
    public float CurrentY { get; set; }
    public float CurrentOpacity { get; set; } = 1.0f;
    public float CurrentScale { get; set; } = 1.0f;
}

public sealed class SujianAnimationController
{
    private readonly List<ActiveAnimation> _activeAnimations = new();
    private bool _animationEnabled = true;
    private const double TimeoutSafetyFactor = 2.0;
    private const double TimeoutSafetyMarginMs = 200.0;

    public bool AnimationEnabled
    {
        get => _animationEnabled;
        set
        {
            if (_animationEnabled == value) return;
            _animationEnabled = value;
            if (!value) ClearAll();
        }
    }

    public IReadOnlyList<ActiveAnimation> ActiveAnimations => _activeAnimations;

    public event EventHandler<AnimationFinishedEventArgs>? AnimationFinished;
    public event EventHandler? AnimationsChanged;

    /// <summary>
    /// 处理一次编辑动画。输入由 SujianEditorHost 从 EditorEditResult + Windows DirectWrite layout 构造。
    /// 不再接收 Core 的 EditorVisualTransaction。
    /// </summary>
    public void ProcessAnimation(WindowsAnimationInput input)
    {
        if (!_animationEnabled || input == null)
        {
            return;
        }

        var animation = new ActiveAnimation
        {
            TransactionId = input.TransactionId,
            Kind = input.Kind,
            StartTime = DateTime.Now,
            DurationMs = input.DurationMs
        };

        switch (input.Kind)
        {
            case WindowsAnimationKind.Insert:
                CreateInsertAnimation(animation, input);
                break;
            case WindowsAnimationKind.Delete:
                CreateDeleteAnimation(animation, input);
                break;
        }

        if (animation.Ghosts.Count > 0)
        {
            _activeAnimations.Add(animation);
            AnimationsChanged?.Invoke(this, EventArgs.Empty);
        }
    }

    private static void CreateInsertAnimation(ActiveAnimation animation, WindowsAnimationInput input)
    {
        if (input.InsertGlyphRects.Count == 0) return;

        float startX = input.OldCursorRect?.X ?? 0f;
        float startY = input.OldCursorRect?.BaselineY ?? 0f;

        foreach (var glyph in input.InsertGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                OriginStartX = startX,
                OriginStartY = startY,
                EndX = glyph.X,
                EndY = glyph.BaselineY,
                Width = glyph.Width,
                Height = glyph.Height,
                BaselineY = glyph.BaselineY,
                AnimKind = GhostAnimKind.Insert,
                CurrentX = startX,
                CurrentY = startY,
                CurrentOpacity = 0.0f,
                CurrentScale = 0.72f
            });
        }

        foreach (var rr in input.ReflowGlyphRects)
        {
            var dx = Math.Abs(rr.NewX - rr.OldX);
            var dy = Math.Abs(rr.NewY - rr.OldY);
            if (dx < 0.5 && dy < 0.5) continue;

            animation.Ghosts.Add(new GhostGlyph
            {
                Char = rr.Char,
                OriginStartX = rr.OldX,
                OriginStartY = rr.OldBaselineY,
                EndX = rr.NewX,
                EndY = rr.NewBaselineY,
                Width = rr.Width,
                Height = rr.Height,
                BaselineY = rr.NewBaselineY,
                AnimKind = GhostAnimKind.Reflow,
                CurrentX = rr.OldX,
                CurrentY = rr.OldBaselineY,
                CurrentOpacity = 1.0f,
                CurrentScale = 1.0f
            });
        }
    }

    private static void CreateDeleteAnimation(ActiveAnimation animation, WindowsAnimationInput input)
    {
        if (input.DeletedGlyphRects.Count == 0) return;

        float endX = input.NewCursorRect?.X ?? 0f;
        float endY = input.NewCursorRect?.BaselineY ?? 0f;

        foreach (var glyph in input.DeletedGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                OriginStartX = glyph.X,
                OriginStartY = glyph.BaselineY,
                EndX = endX,
                EndY = endY,
                Width = glyph.Width,
                Height = glyph.Height,
                BaselineY = glyph.BaselineY,
                AnimKind = GhostAnimKind.Delete,
                CurrentX = glyph.X,
                CurrentY = glyph.BaselineY,
                CurrentOpacity = 1.0f,
                CurrentScale = 1.0f
            });
        }
    }

    public void Tick()
    {
        var now = DateTime.Now;
        var toRemove = new List<ActiveAnimation>();

        foreach (var anim in _activeAnimations)
        {
            var elapsed = (now - anim.StartTime).TotalMilliseconds;
            var progress = Math.Min(1.0, elapsed / anim.DurationMs);
            var timeoutMs = anim.DurationMs * TimeoutSafetyFactor + TimeoutSafetyMarginMs;

            if (elapsed > timeoutMs)
            {
                anim.IsFinished = true;
                toRemove.Add(anim);
                continue;
            }

            foreach (var ghost in anim.Ghosts)
            {
                float p = (float)progress;
                switch (ghost.AnimKind)
                {
                    case GhostAnimKind.Insert:
                        {
                            var eased = EaseOutCubic(p);
                            ghost.CurrentX = ghost.OriginStartX + (ghost.EndX - ghost.OriginStartX) * eased;
                            ghost.CurrentY = ghost.OriginStartY + (ghost.EndY - ghost.OriginStartY) * eased;
                            ghost.CurrentScale = 0.72f + 0.28f * eased;
                            if (p < 0.4f)
                            {
                                ghost.CurrentOpacity = p / 0.4f;
                            }
                            else
                            {
                                ghost.CurrentOpacity = 1.0f - (p - 0.4f) / 0.6f;
                            }
                        }
                        break;
                    case GhostAnimKind.Delete:
                        {
                            var eased = EaseInCubic(p);
                            ghost.CurrentX = ghost.OriginStartX + (ghost.EndX - ghost.OriginStartX) * eased;
                            ghost.CurrentY = ghost.OriginStartY + (ghost.EndY - ghost.OriginStartY) * eased;
                            ghost.CurrentOpacity = 1.0f - eased;
                            ghost.CurrentScale = 1.0f - 0.55f * eased;
                        }
                        break;
                    case GhostAnimKind.Reflow:
                        {
                            var eased = EaseOutCubic(p);
                            ghost.CurrentX = ghost.OriginStartX + (ghost.EndX - ghost.OriginStartX) * eased;
                            ghost.CurrentY = ghost.OriginStartY + (ghost.EndY - ghost.OriginStartY) * eased;
                            ghost.CurrentOpacity = 1.0f;
                            ghost.CurrentScale = 1.0f;
                        }
                        break;
                }
            }

            if (progress >= 1.0)
            {
                anim.IsFinished = true;
                toRemove.Add(anim);
            }
        }

        foreach (var anim in toRemove)
        {
            _activeAnimations.Remove(anim);
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = anim.TransactionId
            });
        }

        if (toRemove.Count > 0)
            AnimationsChanged?.Invoke(this, EventArgs.Empty);
    }

    public void ClearAll()
    {
        foreach (var anim in _activeAnimations)
        {
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = anim.TransactionId
            });
        }
        _activeAnimations.Clear();
        AnimationsChanged?.Invoke(this, EventArgs.Empty);
    }

    private static float EaseOutCubic(float t) => 1.0f - (1.0f - t) * (1.0f - t) * (1.0f - t);
    private static float EaseInCubic(float t) => t * t * t;
}

public sealed class AnimationFinishedEventArgs : EventArgs
{
    public ulong TransactionId { get; set; }
}
