using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using Windows.UI;

namespace Sujian.Windows.Editor.Animation;

public sealed class SujianAnimationOverlay : UserControl
{
    private CanvasControl? _canvas;
    private readonly SujianAnimationController _controller;
    private DispatcherTimer? _tickTimer;
    private Color _textColor = Colors.White;

    public SujianAnimationController Controller => _controller;

    public Color TextColor
    {
        get => _textColor;
        set { _textColor = value; _canvas?.Invalidate(); }
    }

    public SujianAnimationOverlay()
    {
        _controller = new SujianAnimationController();
        _controller.AnimationsChanged += OnAnimationsChanged;

        _canvas = new CanvasControl();
        _canvas.Draw += OnDraw;
        Content = _canvas;

        IsHitTestVisible = false;
        HorizontalAlignment = HorizontalAlignment.Stretch;
        VerticalAlignment = VerticalAlignment.Stretch;

        _tickTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(16) };
        _tickTimer.Tick += (_, _) => { _controller.Tick(); _canvas?.Invalidate(); };
    }

    public void StartTick()
    {
        _tickTimer?.Start();
    }

    public void StopTick()
    {
        _tickTimer?.Stop();
    }

    private void OnAnimationsChanged(object? sender, EventArgs e)
    {
        _canvas?.Invalidate();
    }

    private void OnDraw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var ds = args.DrawingSession;

        foreach (var anim in _controller.ActiveAnimations)
        {
            foreach (var ghost in anim.Ghosts)
            {
                if (ghost.Opacity <= 0.01f) continue;

                var color = Color.FromArgb(
                    (byte)(ghost.Opacity * 255),
                    _textColor.R,
                    _textColor.G,
                    _textColor.B);

                try
                {
                    ds.FillRectangle(ghost.StartX, ghost.StartY - ghost.Height * 0.8f,
                        ghost.Width, ghost.Height, color);
                }
                catch { }
            }
        }
    }
}
