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

        _editor.TextChanged += (s, e) =>
        {
            if (Text != _editor.Text)
                SetValue(TextProperty, _editor.Text);
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
        System.Diagnostics.Debug.WriteLine(
            $"[SujianEditorHost] InputPane showing. Cursor rect: X={cursorRect.X}, Y={cursorRect.Y}");
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
    public void CommitComposition(string text) => _editor.CommitComposition(text);
    public void CancelComposition() => _editor.CancelComposition();
}
