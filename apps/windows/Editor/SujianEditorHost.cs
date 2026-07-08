using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.UI.ViewManagement;
using System;

namespace Sujian.Windows.Editor;

public sealed class SujianEditorHost : UserControl
{
    private readonly SujianEditor _editor;
    private readonly ScrollViewer _scrollViewer;
    private InputPane? _inputPane;
    private DispatcherTimer? _autoSaveTimer;
    private string? _currentProjectId;
    private string? _currentVolumeId;
    private string? _currentChapterId;
    private Bridge.WriterCoreBridge? _core;

    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text), typeof(string), typeof(SujianEditorHost),
        new PropertyMetadata(string.Empty, OnTextChanged));

    public static readonly DependencyProperty FontSizeSettingProperty = DependencyProperty.Register(
        nameof(FontSizeSetting), typeof(float), typeof(SujianEditorHost),
        new PropertyMetadata(16f));

    public static readonly DependencyProperty FirstLineIndentEmProperty = DependencyProperty.Register(
        nameof(FirstLineIndentEm), typeof(float), typeof(SujianEditorHost),
        new PropertyMetadata(2f));

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

    public SujianEditorHost()
    {
        _editor = new SujianEditor();
        _scrollViewer = new ScrollViewer
        {
            Content = _editor,
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
        });

        RegisterPropertyChangedCallback(FirstLineIndentEmProperty, (s, dp) =>
        {
            _editor.FirstLineIndentEm = FirstLineIndentEm;
        });

        Loaded += OnLoaded;
    }

    public event EventHandler? TextChangedByUser;

    public void SetChapterContext(string projectId, string volumeId, string chapterId, Bridge.WriterCoreBridge core)
    {
        _currentProjectId = projectId;
        _currentVolumeId = volumeId;
        _currentChapterId = chapterId;
        _core = core;
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
