using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Sujian.Windows.Bridge;
using System;
using System.Collections.Generic;

namespace Sujian.Windows.Theme;

public sealed class ThemeManager
{
    private static ThemeManager? _instance;
    public static ThemeManager Instance => _instance ??= new ThemeManager();

    private readonly WriterCoreBridge _core = new();
    private List<ThemePaletteRecordDto> _paletteRecords = new();
    private List<BuiltinThemeDto> _builtinThemes = new();
    private ThemeColorSchemeDto? _currentLightScheme;
    private ThemeColorSchemeDto? _currentDarkScheme;
    private string _appearanceMode = "system";
    private string _colorSource = "built_in";
    private string _selectedPaletteId = "";
    private string _selectedBuiltinThemeId = "";

    public event EventHandler? ThemeChanged;

    private ThemeManager() { }

    public async void InitializeAsync()
    {
        try
        {
            var settings = await _core.LoadSettingsAsync();
            _appearanceMode = settings.AppearanceMode;
            _colorSource = settings.ColorSource;
            _selectedPaletteId = settings.SelectedPaletteId;
            _selectedBuiltinThemeId = settings.SelectedBuiltinThemeId;

            try
            {
                _paletteRecords = new List<ThemePaletteRecordDto>(await _core.ListPaletteRecordsAsync());
            }
            catch { }

            try
            {
                _builtinThemes = new List<BuiltinThemeDto>(await _core.ListBuiltinThemesAsync());
            }
            catch { }

            ResolveAndApply();
        }
        catch { }
    }

    public async void UpdateFromSettings(LocalSettings settings)
    {
        _appearanceMode = settings.AppearanceMode;
        _colorSource = settings.ColorSource;
        _selectedPaletteId = settings.SelectedPaletteId;
        _selectedBuiltinThemeId = settings.SelectedBuiltinThemeId;

        try
        {
            _paletteRecords = new List<ThemePaletteRecordDto>(await _core.ListPaletteRecordsAsync());
        }
        catch { }

        try
        {
            _builtinThemes = new List<BuiltinThemeDto>(await _core.ListBuiltinThemesAsync());
        }
        catch { }

        ResolveAndApply();
    }

    private void ResolveAndApply()
    {
        ResolveSchemes();
        ApplyElementTheme();
        ApplyColorResources();
        ThemeChanged?.Invoke(this, EventArgs.Empty);
    }

    private void ResolveSchemes()
    {
        _currentLightScheme = null;
        _currentDarkScheme = null;

        if (_colorSource == "saved_palette" && !string.IsNullOrEmpty(_selectedPaletteId))
        {
            var record = _paletteRecords.Find(r => r.PaletteId == _selectedPaletteId);
            if (record != null)
            {
                _currentLightScheme = record.LightScheme;
                _currentDarkScheme = record.DarkScheme;
                return;
            }
        }

        if (_colorSource == "built_in" && !string.IsNullOrEmpty(_selectedBuiltinThemeId))
        {
            var theme = _builtinThemes.Find(t => t.ThemeId == _selectedBuiltinThemeId);
            if (theme != null)
            {
                _currentLightScheme = theme.LightScheme;
                _currentDarkScheme = theme.DarkScheme;
                return;
            }
        }

        if (_builtinThemes.Count > 0)
        {
            var defaultTheme = _builtinThemes.Find(t => t.ThemeId == "sujian_default") ?? _builtinThemes[0];
            _currentLightScheme = defaultTheme.LightScheme;
            _currentDarkScheme = defaultTheme.DarkScheme;
        }
    }

    private void ApplyElementTheme()
    {
        var theme = _appearanceMode switch
        {
            "light" => ElementTheme.Light,
            "dark" => ElementTheme.Dark,
            _ => ElementTheme.Default
        };
        App.SetTheme(theme);
    }

    private void ApplyColorResources()
    {
        var isDark = IsCurrentlyDark();
        var scheme = isDark ? _currentDarkScheme : _currentLightScheme;
        if (scheme == null) return;

        var appResources = Application.Current.Resources;
        ApplySchemeToResources(scheme, appResources);

        if (App.Current is App app && app.Window?.Content is FrameworkElement root)
        {
            ApplySchemeToResources(scheme, root.Resources);
        }
    }

    private bool IsCurrentlyDark()
    {
        if (_appearanceMode == "dark") return true;
        if (_appearanceMode == "light") return false;
        if (App.Current is App app && app.Window?.Content is FrameworkElement root)
        {
            return root.ActualTheme == ElementTheme.Dark;
        }
        return false;
    }

    private static void ApplySchemeToResources(ThemeColorSchemeDto scheme, ResourceDictionary resources)
    {
        SetBrush(resources, "SujianPrimaryBrush", scheme.Primary);
        SetBrush(resources, "SujianOnPrimaryBrush", scheme.OnPrimary);
        SetBrush(resources, "SujianPrimaryContainerBrush", scheme.PrimaryContainer);
        SetBrush(resources, "SujianOnPrimaryContainerBrush", scheme.OnPrimaryContainer);
        SetBrush(resources, "SujianSecondaryBrush", scheme.Secondary);
        SetBrush(resources, "SujianOnSecondaryBrush", scheme.OnSecondary);
        SetBrush(resources, "SujianSecondaryContainerBrush", scheme.SecondaryContainer);
        SetBrush(resources, "SujianOnSecondaryContainerBrush", scheme.OnSecondaryContainer);
        SetBrush(resources, "SujianTertiaryBrush", scheme.Tertiary);
        SetBrush(resources, "SujianOnTertiaryBrush", scheme.OnTertiary);
        SetBrush(resources, "SujianTertiaryContainerBrush", scheme.TertiaryContainer);
        SetBrush(resources, "SujianOnTertiaryContainerBrush", scheme.OnTertiaryContainer);
        SetBrush(resources, "SujianErrorBrush", scheme.Error);
        SetBrush(resources, "SujianOnErrorBrush", scheme.OnError);
        SetBrush(resources, "SujianErrorContainerBrush", scheme.ErrorContainer);
        SetBrush(resources, "SujianOnErrorContainerBrush", scheme.OnErrorContainer);
        SetBrush(resources, "SujianBackgroundBrush", scheme.Background);
        SetBrush(resources, "SujianOnBackgroundBrush", scheme.OnBackground);
        SetBrush(resources, "SujianSurfaceBrush", scheme.Surface);
        SetBrush(resources, "SujianOnSurfaceBrush", scheme.OnSurface);
        SetBrush(resources, "SujianSurfaceVariantBrush", scheme.SurfaceVariant);
        SetBrush(resources, "SujianOnSurfaceVariantBrush", scheme.OnSurfaceVariant);
        SetBrush(resources, "SujianSurfaceDimBrush", scheme.SurfaceDim);
        SetBrush(resources, "SujianSurfaceBrightBrush", scheme.SurfaceBright);
        SetBrush(resources, "SujianSurfaceContainerLowestBrush", scheme.SurfaceContainerLowest);
        SetBrush(resources, "SujianSurfaceContainerLowBrush", scheme.SurfaceContainerLow);
        SetBrush(resources, "SujianSurfaceContainerBrush", scheme.SurfaceContainer);
        SetBrush(resources, "SujianSurfaceContainerHighBrush", scheme.SurfaceContainerHigh);
        SetBrush(resources, "SujianSurfaceContainerHighestBrush", scheme.SurfaceContainerHighest);
        SetBrush(resources, "SujianInverseSurfaceBrush", scheme.InverseSurface);
        SetBrush(resources, "SujianInverseOnSurfaceBrush", scheme.InverseOnSurface);
        SetBrush(resources, "SujianInversePrimaryBrush", scheme.InversePrimary);
        SetBrush(resources, "SujianOutlineBrush", scheme.Outline);
        SetBrush(resources, "SujianOutlineVariantBrush", scheme.OutlineVariant);
        SetBrush(resources, "SujianScrimBrush", scheme.Scrim);
        SetBrush(resources, "SujianSurfaceTintBrush", scheme.SurfaceTint);
    }

    private static void SetBrush(ResourceDictionary resources, string key, string hex)
    {
        if (string.IsNullOrEmpty(hex)) return;
        try
        {
            var color = ParseHexColor(hex);
            resources[key] = new SolidColorBrush(color);
        }
        catch { }
    }

    private static Windows.UI.Color ParseHexColor(string hex)
    {
        hex = hex.TrimStart('#');
        if (hex.Length == 6)
        {
            return Windows.UI.Color.FromArgb(255,
                Convert.ToByte(hex.Substring(0, 2), 16),
                Convert.ToByte(hex.Substring(2, 2), 16),
                Convert.ToByte(hex.Substring(4, 2), 16));
        }
        if (hex.Length == 8)
        {
            return Windows.UI.Color.FromArgb(
                Convert.ToByte(hex.Substring(0, 2), 16),
                Convert.ToByte(hex.Substring(2, 2), 16),
                Convert.ToByte(hex.Substring(4, 2), 16),
                Convert.ToByte(hex.Substring(6, 2), 16));
        }
        return Windows.UI.Color.FromArgb(255, 0, 0, 0);
    }
}
