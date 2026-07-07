using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sujian.Windows.Bridge;

namespace Sujian.Windows.Pages;

public sealed partial class SettingsPage : Page
{
    private readonly WriterCoreBridge _core = new();

    public SettingsPage()
    {
        InitializeComponent();
        LoadSettings();
    }

    private async void LoadSettings()
    {
        try
        {
            var settings = await _core.LoadSettingsAsync();
            FontSizeBox.Value = settings.FontSize;
            LineHeightBox.Value = settings.LineHeight;
            IndentBox.Value = 2;
            AutoSaveToggle.IsOn = settings.AutoSave;
            AutoIndentToggle.IsOn = settings.AutoIndent;
            ThemeRadio.SelectedIndex = settings.Theme switch
            {
                "light" => 1,
                "dark" => 2,
                _ => 0
            };
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"加载设置失败: {ex.Code}";
        }
    }

    private async void SaveSettings_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var theme = ThemeRadio.SelectedIndex switch
            {
                1 => "light",
                2 => "dark",
                _ => "system"
            };
            var settings = new LocalSettings
            {
                FontSize = (float)FontSizeBox.Value,
                LineHeight = (float)LineHeightBox.Value,
                Theme = theme,
                AutoSave = AutoSaveToggle.IsOn,
                AutoIndent = AutoIndentToggle.IsOn,
            };
            await _core.SaveSettingsAsync(settings);
            StatusText.Text = "设置已保存";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"保存失败: {ex.Code}";
        }
    }
}
