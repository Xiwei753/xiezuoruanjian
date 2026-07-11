using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace Sujian.Windows.Editor;

public sealed class WindowsEditorThemeAdapter
{
    private static WindowsEditorThemeAdapter? _instance;
    public static WindowsEditorThemeAdapter Instance => _instance ??= new();

    public Color BackgroundColor { get; private set; } = Color.FromArgb(255, 255, 251, 254);
    public Color TextColor { get; private set; } = Color.FromArgb(255, 28, 27, 31);
    public Color CursorColor { get; private set; } = Color.FromArgb(255, 103, 80, 164);
    public Color SelectionColor { get; private set; } = Color.FromArgb(76, 103, 80, 164);
    public Color PreeditColor { get; private set; } = Color.FromArgb(255, 30, 144, 255);
    public Color BorderColor { get; private set; } = Color.FromArgb(255, 121, 116, 126);

    private WindowsEditorThemeAdapter()
    {
        Theme.ThemeManager.Instance.ThemeChanged += OnThemeChanged;
        RefreshFromResources();
    }

    private void OnThemeChanged(object? sender, EventArgs e)
    {
        RefreshFromResources();
    }

    public void RefreshFromResources()
    {
        var resources = Application.Current.Resources;

        BackgroundColor = GetBrushColor(resources, "SujianSurfaceBrush", BackgroundColor);
        TextColor = GetBrushColor(resources, "SujianOnSurfaceBrush", TextColor);
        CursorColor = GetBrushColor(resources, "SujianPrimaryBrush", CursorColor);
        SelectionColor = Color.FromArgb(76, CursorColor.R, CursorColor.G, CursorColor.B);
        PreeditColor = GetBrushColor(resources, "SujianTertiaryBrush", PreeditColor);
        BorderColor = GetBrushColor(resources, "SujianOutlineBrush", BorderColor);
    }

    private static Color GetBrushColor(ResourceDictionary resources, string key, Color fallback)
    {
        try
        {
            if (resources[key] is SolidColorBrush brush)
            {
                return brush.Color;
            }
        }
        catch { }
        return fallback;
    }
}
