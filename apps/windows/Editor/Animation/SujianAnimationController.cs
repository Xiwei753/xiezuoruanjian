using System;
using System.Collections.Generic;

namespace Sujian.Windows.Editor.Animation;

public enum GhostAnimKind
{
    Insert,
    Delete,
    Reflow
}

public sealed class ActiveAnimation
{
    public ulong TransactionId { get; set; }
    public ulong? RangeId { get; set; }
    public EditorAnimationKind Kind { get; set; }
    public AnimationMode Mode { get; set; }
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

    public void ProcessTransaction(EditorVisualTransaction vt)
    {
        if (!_animationEnabled || vt.ParsedAnimationMode == AnimationMode.SystemSuppressed)
        {
            NotifySkipped(vt);
            return;
        }

        var animation = new ActiveAnimation
        {
            TransactionId = vt.Id,
            Kind = vt.ParsedKind,
            Mode = vt.ParsedAnimationMode,
            StartTime = DateTime.Now,
            DurationMs = vt.DurationMs
        };

        if (vt.HiddenVisualRanges.Count > 0)
            animation.RangeId = vt.HiddenVisualRanges[0].Id;

        switch (vt.ParsedKind)
        {
            case EditorAnimationKind.Insert:
                CreateInsertAnimation(animation, vt);
                break;
            case EditorAnimationKind.Delete:
                CreateDeleteAnimation(animation, vt);
                break;
        }

        if (animation.Ghosts.Count > 0)
        {
            _activeAnimations.Add(animation);
            AnimationsChanged?.Invoke(this, EventArgs.Empty);
        }
        else
        {
            NotifySkipped(vt);
        }
    }

    private void CreateInsertAnimation(ActiveAnimation animation, EditorVisualTransaction vt)
    {
        if (vt.InsertGlyphRects == null || vt.InsertGlyphRects.Count == 0) return;

        float startX = (float)(vt.OldCursorRect?.X ?? 0);
        float startY = (float)(vt.OldCursorRect?.BaselineY ?? 0);

        foreach (var glyph in vt.InsertGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                OriginStartX = startX,
                OriginStartY = startY,
                EndX = (float)glyph.X,
                EndY = (float)glyph.BaselineY,
                Width = (float)glyph.W,
                Height = (float)glyph.H,
                BaselineY = (float)glyph.BaselineY,
                AnimKind = GhostAnimKind.Insert,
                CurrentX = startX,
                CurrentY = startY,
                CurrentOpacity = 0.0f,
                CurrentScale = 0.72f
            });
        }

        if (vt.ReflowGlyphRects != null)
        {
            foreach (var rr in vt.ReflowGlyphRects)
            {
                var dx = Math.Abs(rr.NewX - rr.OldX);
                var dy = Math.Abs(rr.NewY - rr.OldY);
                if (dx < 0.5 && dy < 0.5) continue;

                animation.Ghosts.Add(new GhostGlyph
                {
                    Char = rr.Char,
                    OriginStartX = (float)rr.OldX,
                    OriginStartY = (float)(rr.OldBaselineY ?? rr.OldY),
                    EndX = (float)rr.NewX,
                    EndY = (float)(rr.NewBaselineY ?? rr.NewY),
                    Width = (float)rr.W,
                    Height = (float)rr.H,
                    BaselineY = (float)(rr.NewBaselineY ?? 0),
                    AnimKind = GhostAnimKind.Reflow,
                    CurrentX = (float)rr.OldX,
                    CurrentY = (float)(rr.OldBaselineY ?? rr.OldY),
                    CurrentOpacity = 1.0f,
                    CurrentScale = 1.0f
                });
            }
        }
    }

    private void CreateDeleteAnimation(ActiveAnimation animation, EditorVisualTransaction vt)
    {
        if (vt.DeletedGlyphRects == null || vt.DeletedGlyphRects.Count == 0) return;

        float endX = (float)(vt.NewCursorRect?.X ?? 0);
        float endY = (float)(vt.NewCursorRect?.BaselineY ?? 0);

        foreach (var glyph in vt.DeletedGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                OriginStartX = (float)glyph.X,
                OriginStartY = (float)glyph.BaselineY,
                EndX = endX,
                EndY = endY,
                Width = (float)glyph.W,
                Height = (float)glyph.H,
                BaselineY = (float)glyph.BaselineY,
                AnimKind = GhostAnimKind.Delete,
                CurrentX = (float)glyph.X,
                CurrentY = (float)glyph.BaselineY,
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
                TransactionId = anim.TransactionId,
                RangeId = anim.RangeId
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
                TransactionId = anim.TransactionId,
                RangeId = anim.RangeId
            });
        }
        _activeAnimations.Clear();
        AnimationsChanged?.Invoke(this, EventArgs.Empty);
    }

    private void NotifySkipped(EditorVisualTransaction vt)
    {
        foreach (var range in vt.HiddenVisualRanges)
        {
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = vt.Id,
                RangeId = range.Id
            });
        }
    }

    private static float EaseOutCubic(float t) => 1.0f - (1.0f - t) * (1.0f - t) * (1.0f - t);
    private static float EaseInCubic(float t) => t * t * t;
}

public sealed class AnimationFinishedEventArgs : EventArgs
{
    public ulong TransactionId { get; set; }
    public ulong? RangeId { get; set; }
}
