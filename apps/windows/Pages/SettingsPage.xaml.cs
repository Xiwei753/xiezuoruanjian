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

        CoordinatedCursorAnimToggle.Toggled += OnCoordinatedCursorAnimToggled;
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
            TypingAnimToggle.IsOn = settings.TypingAnimationEnabled;
            TypingAnimDurationBox.Value = settings.TypingAnimationDurationMs;
            CoordinatedCursorAnimToggle.IsOn = settings.CoordinatedTextCursorAnimationEnabled;
            CoordinatedDurationBox.Value = settings.TypingAnimationDurationMs;
            UpdateAnimationVisibility();
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

    private void OnCoordinatedCursorAnimToggled(object sender, RoutedEventArgs e)
    {
        UpdateAnimationVisibility();
        if (CoordinatedCursorAnimToggle.IsOn)
        {
            TypingAnimToggle.IsOn = true;
            var dur = TypingAnimDurationBox.Value;
            CoordinatedDurationBox.Value = dur;
        }
    }

    private void UpdateAnimationVisibility()
    {
        var coordinated = CoordinatedCursorAnimToggle.IsOn;
        TypingAnimToggle.Visibility = coordinated ? Visibility.Collapsed : Visibility.Visible;
        TypingAnimDurationBox.Visibility = coordinated ? Visibility.Collapsed : Visibility.Visible;
        CoordinatedDurationBox.Visibility = coordinated ? Visibility.Visible : Visibility.Collapsed;
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
            var typingAnimDuration = CoordinatedCursorAnimToggle.IsOn
                ? (int)CoordinatedDurationBox.Value
                : (int)TypingAnimDurationBox.Value;
            var settings = new LocalSettings
            {
                FontSize = (float)FontSizeBox.Value,
                LineHeight = (float)LineHeightBox.Value,
                Theme = theme,
                AutoSave = AutoSaveToggle.IsOn,
                AutoIndent = AutoIndentToggle.IsOn,
                TypingAnimationEnabled = TypingAnimToggle.IsOn,
                TypingAnimationDurationMs = typingAnimDuration,
                CoordinatedTextCursorAnimationEnabled = CoordinatedCursorAnimToggle.IsOn,
                SmoothCursorEnabled = CoordinatedCursorAnimToggle.IsOn || TypingAnimToggle.IsOn,
                SmoothCursorDurationMs = typingAnimDuration,
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
