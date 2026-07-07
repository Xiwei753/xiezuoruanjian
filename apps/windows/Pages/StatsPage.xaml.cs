using Microsoft.UI.Xaml.Controls;
using Sujian.Windows.Bridge;

namespace Sujian.Windows.Pages;

public sealed partial class StatsPage : Page
{
    private readonly WriterCoreBridge _core = new();

    public StatsPage()
    {
        InitializeComponent();
        LoadStats();
    }

    private async void LoadStats()
    {
        try
        {
            var stats = await _core.GetWritingStatsAsync();
            TotalWordsText.Text = stats.TotalWords.ToString("N0");
            TodayWordsText.Text = stats.TodayWords.ToString("N0");
            SessionWordsText.Text = stats.SessionWords.ToString("N0");
            StreakDaysText.Text = stats.StreakDays.ToString();
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"加载统计失败: {ex.Code}";
        }
    }
}
