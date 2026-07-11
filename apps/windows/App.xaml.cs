using Microsoft.UI.Xaml;
using Sujian.Windows.Theme;

namespace Sujian.Windows;

public partial class App : Application
{
    private Window? _window;

    public Window? Window => _window;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        _window.Activate();
        ThemeManager.Instance.InitializeAsync();
    }

    public static void SetTheme(ElementTheme theme)
    {
        if (Current._window?.Content is FrameworkElement root)
        {
            root.RequestedTheme = theme;
        }
    }
}
