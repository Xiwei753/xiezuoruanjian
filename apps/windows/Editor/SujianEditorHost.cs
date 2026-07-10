using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.UI.ViewManagement;
using System;
using System.Text.Json;
using Sujian.Windows.Editor.Animation;

namespace Sujian.Windows.Editor;

public sealed class SujianEditorHost : UserControl
{
    private readonly SujianEditor _editor;
    private readonly SujianAnimationOverlay _animationOverlay;
    private readonly ScrollViewer _scrollViewer;
    private InputPane? _inputPane;
    private DispatcherTimer? _autoSaveTimer;
    private string? _currentProjectId;
    private string? _currentVolumeId;
    private string? _currentChapterId;
    private Bridge.WriterCoreBridge? _core;
    private bool _typingAnimationEnabled = true;
    private bool _coordinatedCursorAnimationEnabled = true;

    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text), typeof(string), typeof(SujianEditorHost),
        new PropertyMetadata(string.Empty, OnTextChanged));

    public static readonly DependencyProperty FontSizeSettingProperty = DependencyProperty.Register(
        nameof(FontSizeSetting), typeof(float), typeof(SujianEditorHost),
        new PropertyMetadata(16f));

    public static readonly DependencyProperty FirstLineIndentEmProperty = DependencyProperty.Register(
        nameof(FirstLineIndentEm), typeof(float), typeof(SujianEditorHost),
        new PropertyMetadata(2f));

    public static readonly DependencyProperty TypingAnimationEnabledProperty = DependencyProperty.Register(
        nameof(TypingAnimationEnabled), typeof(bool), typeof(SujianEditorHost),
        new PropertyMetadata(true, OnTypingAnimationEnabledChanged));

    public static readonly DependencyProperty CoordinatedCursorAnimationEnabledProperty = DependencyProperty.Register(
        nameof(CoordinatedCursorAnimationEnabled), typeof(bool), typeof(SujianEditorHost),
        new PropertyMetadata(true));

    public static readonly DependencyProperty TypingAnimationDurationMsProperty = DependencyProperty.Register(
        nameof(TypingAnimationDurationMs), typeof(int), typeof(SujianEditorHost),
        new PropertyMetadata(100));

    public string Text
    {
        get => (string)GetValue(TextProperty);
        set => SetValue(TextProperty, value);
    }

    public float FontSizeSetting
    {
        get => (float)GetValue(FontSizeSettingProperty);
        set => SetValue(FontSizeSettingProperty, value);
    }

    public float FirstLineIndentEm
    {
        get => (float)GetValue(FirstLineIndentEmProperty);
        set => SetValue(FirstLineIndentEmProperty, value);
    }

    public bool TypingAnimationEnabled
    {
        get => (bool)GetValue(TypingAnimationEnabledProperty);
        set => SetValue(TypingAnimationEnabledProperty, value);
    }

    public bool CoordinatedCursorAnimationEnabled
    {
        get => (bool)GetValue(CoordinatedCursorAnimationEnabledProperty);
        set => SetValue(CoordinatedCursorAnimationEnabledProperty, value);
    }

    public int TypingAnimationDurationMs
    {
        get => (int)GetValue(TypingAnimationDurationMsProperty);
        set => SetValue(TypingAnimationDurationMsProperty, value);
    }

    public SujianEditorHost()
    {
        _editor = new SujianEditor();
        _animationOverlay = new SujianAnimationOverlay();
        _animationOverlay.Controller.AnimationFinished += OnAnimationFinished;

        var grid = new Grid();
        grid.Children.Add(_editor);
        grid.Children.Add(_animationOverlay);

        _scrollViewer = new ScrollViewer
        {
            Content = grid,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        };
        Content = _scrollViewer;

        _editor.TextChangedByUser += (s, e) =>
        {
            if (Text != _editor.Text)
                SetValue(TextProperty, _editor.Text);
            TextChangedByUser?.Invoke(this, EventArgs.Empty);
        };

        RegisterPropertyChangedCallback(TextProperty, (s, dp) =>
        {
            if (_editor.Text != Text)
                _editor.Text = Text;
        });

        RegisterPropertyChangedCallback(FontSizeSettingProperty, (s, dp) =>
        {
            _editor.FontSizeSetting = FontSizeSetting;
            _animationOverlay.FontSize = FontSizeSetting;
        });

        RegisterPropertyChangedCallback(FirstLineIndentEmProperty, (s, dp) =>
        {
            _editor.FirstLineIndentEm = FirstLineIndentEm;
        });

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    public event EventHandler? TextChangedByUser;

    public void SetChapterContext(string projectId, string volumeId, string chapterId, Bridge.WriterCoreBridge core)
    {
        _currentProjectId = projectId;
        _currentVolumeId = volumeId;
        _currentChapterId = chapterId;
        _core = core;
    }

    public void ApplyAnimationSettings(bool enabled, bool coordinatedEnabled, int durationMs)
    {
        _typingAnimationEnabled = enabled;
        _coordinatedCursorAnimationEnabled = coordinatedEnabled;
        _animationOverlay.Controller.AnimationEnabled = enabled;
    }

    public void ProcessVisualTransaction(string oldText, string newText,
        uint oldCursorIndex, uint newCursorIndex, string cause,
        uint maxAnimatedChars = 20, uint animationDurationMs = 300)
    {
        if (!_typingAnimationEnabled || _core == null) return;

        try
        {
            var json = _core.GetEditorVisualTransaction(
                oldText, newText, oldCursorIndex, newCursorIndex,
                cause, maxAnimatedChars, animationDurationMs);
            if (string.IsNullOrEmpty(json)) return;

            var env = ParseEnvelope(json);
            if (env.Data == null) return;

            var vt = EditorVisualTransaction.FromJson(json);
            if (vt == null) return;

            _animationOverlay.FontSize = FontSizeSetting;
            _animationOverlay.Controller.ProcessTransaction(vt);
            _animationOverlay.StartTick();
        }
        catch { }
    }

    private static EnvelopeResult ParseEnvelope(string? json)
    {
        if (string.IsNullOrEmpty(json)) return new EnvelopeResult { Ok = false, Error = "empty" };
        try
        {
            using var doc = System.Text.Json.JsonDocument.Parse(json!);
            var root = doc.RootElement;
            var ok = root.TryGetProperty("ok", out var okEl) && okEl.GetBoolean();
            var error = root.TryGetProperty("error", out var errEl) ? errEl.GetString() : null;
            var data = root.TryGetProperty("data", out var dataEl) ? dataEl : (JsonElement?)null;
            return new EnvelopeResult { Ok = ok, Error = error, Data = data };
        }
        catch
        {
            return new EnvelopeResult { Ok = false, Error = "parse_error" };
        }
    }

    private sealed class EnvelopeResult
    {
        public bool Ok;
        public string? Error;
        public JsonElement? Data;
    }

    private void OnAnimationFinished(object? sender, AnimationFinishedEventArgs e)
    {
        if (_animationOverlay.Controller.ActiveAnimations.Count == 0)
        {
            _animationOverlay.StopTick();
        }
    }

    private static void OnTypingAnimationEnabledChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var host = (SujianEditorHost)d;
        host._typingAnimationEnabled = (bool)e.NewValue;
        host._animationOverlay.Controller.AnimationEnabled = (bool)e.NewValue;
        if (!(bool)e.NewValue)
        {
            host._animationOverlay.Controller.ClearAll();
            host._animationOverlay.StopTick();
        }
    }

    public void EnableAutoSave(int intervalSeconds = 30)
    {
        DisableAutoSave();
        _autoSaveTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(intervalSeconds) };
        _autoSaveTimer.Tick += OnAutoSaveTick;
        _autoSaveTimer.Start();
    }

    public void DisableAutoSave()
    {
        _autoSaveTimer?.Stop();
        _autoSaveTimer = null;
    }

    private async void OnAutoSaveTick(object? sender, object e)
    {
        if (_core == null || _currentProjectId == null || _currentVolumeId == null || _currentChapterId == null)
            return;
        try
        {
            await _core.SaveChapterAsync(_currentProjectId, _currentVolumeId, _currentChapterId, _editor.Text);
        }
        catch { }
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        try
        {
            _inputPane = InputPane.GetForCurrentView();
            _inputPane.Showing += OnInputPaneShowing;
        }
        catch { }
        _animationOverlay.FontSize = FontSizeSetting;
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        _animationOverlay.StopTick();
        _animationOverlay.Controller.ClearAll();
    }

    private void OnInputPaneShowing(InputPane sender, InputPaneVisibilityEventArgs args)
    {
        var cursorRect = _editor.GetCursorRect();
        var editorBottom = _editor.ActualHeight;
        if (cursorRect.Y + cursorRect.Height > editorBottom - sender.OccludedRect.Height)
        {
            var delta = cursorRect.Y + cursorRect.Height - (editorBottom - sender.OccludedRect.Height);
            _scrollViewer.ChangeView(null, _scrollViewer.VerticalOffset + delta, null);
        }
    }

    public Rect GetCursorRectForIME()
    {
        var rect = _editor.GetCursorRect();
        var transform = _editor.TransformToVisual(this);
        var point = transform.TransformPoint(new Windows.Foundation.Point(rect.X, rect.Y));
        return new Rect(point.X, point.Y, rect.Width, rect.Height);
    }

    public void StartComposition() => _editor.StartComposition();
    public void UpdateComposition(string text) => _editor.UpdateComposition(text);
    public void UpdateComposition(string text, int cursor) => _editor.UpdateComposition(text, cursor);
    public void CommitComposition(string text) => _editor.CommitComposition(text);
    public void CancelComposition() => _editor.CancelComposition();
}
