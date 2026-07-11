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

        if (App.Current is App app && app.Window?.Content is FrameworkElement root)
        {
            var resources = root.Resources;
            ApplySchemeToResources(scheme, resources);
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
        SetColor(resources, "SujianPrimary", scheme.Primary);
        SetColor(resources, "SujianOnPrimary", scheme.OnPrimary);
        SetColor(resources, "SujianPrimaryContainer", scheme.PrimaryContainer);
        SetColor(resources, "SujianOnPrimaryContainer", scheme.OnPrimaryContainer);
        SetColor(resources, "SujianSecondary", scheme.Secondary);
        SetColor(resources, "SujianOnSecondary", scheme.OnSecondary);
        SetColor(resources, "SujianSecondaryContainer", scheme.SecondaryContainer);
        SetColor(resources, "SujianOnSecondaryContainer", scheme.OnSecondaryContainer);
        SetColor(resources, "SujianTertiary", scheme.Tertiary);
        SetColor(resources, "SujianOnTertiary", scheme.OnTertiary);
        SetColor(resources, "SujianTertiaryContainer", scheme.TertiaryContainer);
        SetColor(resources, "SujianOnTertiaryContainer", scheme.OnTertiaryContainer);
        SetColor(resources, "SujianError", scheme.Error);
        SetColor(resources, "SujianOnError", scheme.OnError);
        SetColor(resources, "SujianErrorContainer", scheme.ErrorContainer);
        SetColor(resources, "SujianOnErrorContainer", scheme.OnErrorContainer);
        SetColor(resources, "SujianBackground", scheme.Background);
        SetColor(resources, "SujianOnBackground", scheme.OnBackground);
        SetColor(resources, "SujianSurface", scheme.Surface);
        SetColor(resources, "SujianOnSurface", scheme.OnSurface);
        SetColor(resources, "SujianSurfaceVariant", scheme.SurfaceVariant);
        SetColor(resources, "SujianOnSurfaceVariant", scheme.OnSurfaceVariant);
        SetColor(resources, "SujianSurfaceDim", scheme.SurfaceDim);
        SetColor(resources, "SujianSurfaceBright", scheme.SurfaceBright);
        SetColor(resources, "SujianSurfaceContainerLowest", scheme.SurfaceContainerLowest);
        SetColor(resources, "SujianSurfaceContainerLow", scheme.SurfaceContainerLow);
        SetColor(resources, "SujianSurfaceContainer", scheme.SurfaceContainer);
        SetColor(resources, "SujianSurfaceContainerHigh", scheme.SurfaceContainerHigh);
        SetColor(resources, "SujianSurfaceContainerHighest", scheme.SurfaceContainerHighest);
        SetColor(resources, "SujianInverseSurface", scheme.InverseSurface);
        SetColor(resources, "SujianInverseOnSurface", scheme.InverseOnSurface);
        SetColor(resources, "SujianInversePrimary", scheme.InversePrimary);
        SetColor(resources, "SujianOutline", scheme.Outline);
        SetColor(resources, "SujianOutlineVariant", scheme.OutlineVariant);
        SetColor(resources, "SujianScrim", scheme.Scrim);
    }

    private static void SetColor(ResourceDictionary resources, string key, string hex)
    {
        if (string.IsNullOrEmpty(hex)) return;
        try
        {
            var color = ParseHexColor(hex);
            resources[key] = color;
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
