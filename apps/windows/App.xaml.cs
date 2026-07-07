using Microsoft.UI.Xaml;

namespace Sujian.Windows;

public partial class App : Application
{
    private Window? _window;

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        _window.Activate();
    }

    public static void SetTheme(ElementTheme theme)
    {
        if (Current._window?.Content is FrameworkElement root)
        {
            root.RequestedTheme = theme;
        }
    }
}
