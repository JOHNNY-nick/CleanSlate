using System.Globalization;
using System.Xml.Linq;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// Themes Clair, Sombre and Multicolore (D71, D72). The owner wants Multicolore to put « plenty
/// of colors everywhere »: red, yellow, green, blue and violet, while every text stays readable.
/// </summary>
public sealed class ThemeTests
{
    private static readonly XNamespace Xaml = "http://schemas.microsoft.com/winfx/2006/xaml/presentation";
    private static readonly XNamespace X = "http://schemas.microsoft.com/winfx/2006/xaml";

    // Colors live in App.xaml (CLAUDE.md: no hard-coded color); read from there.
    private static readonly XElement Palette = ReadPalette();

    private static XElement ReadPalette()
    {
        string? dir = AppContext.BaseDirectory;
        while (dir is not null && !File.Exists(Path.Combine(dir, "app", "CleanSlate.App", "App.xaml")))
        {
            dir = Path.GetDirectoryName(dir);
        }

        return XDocument.Load(Path.Combine(dir!, "app", "CleanSlate.App", "App.xaml"))
            .Descendants(Xaml + "ResourceDictionary")
            .Single(d => (string?)d.Attribute(X + "Key") == ThemePalette.ColorfulKey);
    }

    private static XElement Entry(string key) =>
        Palette.Elements().Single(e => (string?)e.Attribute(X + "Key") == key);

    /// <summary>The color of a solid token, or every stop of a gradient token.</summary>
    private static string[] Colors(string key)
    {
        XElement e = Entry(key);
        return e.Name.LocalName switch
        {
            "Color" => [e.Value.Trim()],
            "LinearGradientBrush" => e.Descendants(Xaml + "GradientStop").Select(s => (string)s.Attribute("Color")!).ToArray(),
            _ => throw new InvalidOperationException($"{key}: {e.Name.LocalName}"),
        };
    }

    [Fact]
    public void EveryTokenOfTheTheme_IsInThePalette()
    {
        Assert.All(ThemePalette.Tokens, t => Assert.NotNull(Palette.Elements().SingleOrDefault(e => (string?)e.Attribute(X + "Key") == t)));
        Assert.Equal("LinearGradientBrush", Entry("WindowBackgroundBrush").Name.LocalName);
        Assert.Equal("LinearGradientBrush", Entry("BorderBrush").Name.LocalName);
        Assert.Equal("LinearGradientBrush", Entry("TitleBrush").Name.LocalName);
    }

    /// <summary>Every token changed by Multicolore exists in the three other dictionaries, so that every theme has it (D72).</summary>
    [Theory]
    [InlineData("Light")]
    [InlineData("Dark")]
    [InlineData("HighContrast")]
    public void EveryToken_ExistsInEveryTheme(string dictionary)
    {
        XElement d = ThemeDictionary(dictionary);
        // In high contrast, the accent controls keep the system colors of WinUI on purpose.
        IEnumerable<string> tokens = dictionary == "HighContrast"
            ? ThemePalette.Tokens.Where(t => !t.StartsWith("AccentButton", StringComparison.Ordinal) && !t.StartsWith("Accent", StringComparison.Ordinal) || t is "AccentBrush" or "AccentHoverBrush" or "AccentSubtleBrush")
            : ThemePalette.Tokens;
        Assert.All(tokens, t => Assert.True(d.Elements().Any(e => (string?)e.Attribute(X + "Key") == t), $"{dictionary} : {t}"));
    }

    [Fact]
    public void Multicolore_HasRedYellowGreenBlueAndViolet()
    {
        double[] hues = Palette.Descendants()
            .SelectMany(e => e.Name.LocalName == "GradientStop" ? [(string)e.Attribute("Color")!] : e.Name.LocalName == "Color" ? [e.Value.Trim()] : Array.Empty<string>())
            .Where(c => Saturation(c) >= 0.5)
            .Select(Hue)
            .ToArray();
        (string Name, double From, double To)[] families =
            [("rouge", 345, 375), ("jaune", 40, 65), ("vert", 90, 160), ("bleu", 200, 235), ("violet", 255, 300)];
        Assert.All(families, f => Assert.True(
            hues.Any(h => (h >= f.From && h <= f.To) || (h + 360 >= f.From && h + 360 <= f.To)),
            $"pas de {f.Name}"));
    }

    [Theory]
    [InlineData("TextPrimaryBrush", "WindowBackgroundBrush", 7.0)]
    [InlineData("TextSecondaryBrush", "WindowBackgroundBrush", 4.5)]
    [InlineData("TextPrimaryBrush", "SurfaceBrush", 7.0)]
    [InlineData("TextSecondaryBrush", "SurfaceBrush", 4.5)]
    [InlineData("TitleBrush", "WindowBackgroundBrush", 3.0)] // titles are large text
    [InlineData("AccentButtonForeground", "AccentButtonBackground", 4.5)]
    [InlineData("AccentButtonForeground", "AccentButtonBackgroundPointerOver", 4.5)]
    [InlineData("DangerBrush", "SurfaceBrush", 4.5)]
    [InlineData("ConfidenceHighForegroundBrush", "ConfidenceHighBackgroundBrush", 4.5)]
    [InlineData("ConfidenceMediumForegroundBrush", "ConfidenceMediumBackgroundBrush", 4.5)]
    [InlineData("ConfidenceLowForegroundBrush", "ConfidenceLowBackgroundBrush", 4.5)]
    [InlineData("OnDangerBrush", "DangerBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderHomeBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderAppsBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderOrphansBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderHistoryBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderStorageBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderSettingsBrush", 4.5)]
    [InlineData("PageHeaderForegroundBrush", "PageHeaderWizardBrush", 4.5)]
    [InlineData("TextPrimaryBrush", "CardTint0", 7.0)]
    [InlineData("TextPrimaryBrush", "CardTint1", 7.0)]
    [InlineData("TextPrimaryBrush", "CardTint2", 7.0)]
    [InlineData("TextPrimaryBrush", "CardTint3", 7.0)]
    [InlineData("TextPrimaryBrush", "CardTint4", 7.0)]
    [InlineData("TextSecondaryBrush", "CardTint0", 4.5)]
    [InlineData("TextSecondaryBrush", "CardTint1", 4.5)]
    [InlineData("TextSecondaryBrush", "CardTint2", 4.5)]
    [InlineData("TextSecondaryBrush", "CardTint3", 4.5)]
    [InlineData("TextSecondaryBrush", "CardTint4", 4.5)]
    [InlineData("TextPrimaryBrush", "ContentSurfaceBrush", 7.0)]
    public void EveryText_StaysReadable_OnEveryColorOfItsBackground(string text, string background, double minimum)
    {
        foreach (string t in Colors(text))
        {
            foreach (string b in Colors(background))
            {
                double ratio = ThemePalette.Contrast(t, b);
                Assert.True(ratio >= minimum, $"{text} {t} on {background} {b}: {ratio:0.00}");
            }
        }
    }

    [Fact]
    public void Contrast_FollowsTheWcagFormula()
    {
        Assert.Equal(21.0, ThemePalette.Contrast("#000000", "#FFFFFF"), 2);
        Assert.Equal(1.0, ThemePalette.Contrast("#6C4DFF", "#6C4DFF"), 2);
    }

    [Theory]
    [InlineData(AppTheme.System, 0)]
    [InlineData(AppTheme.Light, 1)]
    [InlineData(AppTheme.Dark, 2)]
    [InlineData(AppTheme.Colorful, 3)]
    public void TheSettings_OfferSystemeClairSombreMulticolore(AppTheme theme, int index)
    {
        Assert.Equal(index, ThemePalette.IndexOf(theme));
        Assert.Equal(theme, ThemePalette.FromIndex(index));
    }

    /// <summary>
    /// D72, badges invisible: their brushes were looked up in Windows' theme. A light theme on a
    /// dark Windows got the dark badges; the dictionary now follows the theme chosen in CleanSlate.
    /// </summary>
    [Theory]
    [InlineData(AppTheme.Light, true, "Light")]
    [InlineData(AppTheme.Colorful, true, "Light")]
    [InlineData(AppTheme.Dark, false, "Dark")]
    [InlineData(AppTheme.System, true, "Dark")]
    [InlineData(AppTheme.System, false, "Light")]
    public void Badges_UseTheChosenTheme_NotWindowsOne(AppTheme chosen, bool windowsIsDark, string dictionary) =>
        Assert.Equal(dictionary, ThemePalette.DictionaryKey(chosen, windowsIsDark));

    /// <summary>Each badge shows its text readably and stands out from the card it sits on (D72).</summary>
    [Theory]
    [InlineData("Light")]
    [InlineData("Dark")]
    [InlineData(ThemePalette.ColorfulKey)]
    public void Badges_AreVisibleInEveryTheme(string dictionary)
    {
        XElement d = ThemeDictionary(dictionary);
        string surface = ColorOf(d, "SurfaceBrush")[0];
        foreach (string level in new[] { "High", "Medium", "Low" })
        {
            string fg = ColorOf(d, $"Confidence{level}ForegroundBrush")[0];
            string bg = ColorOf(d, $"Confidence{level}BackgroundBrush")[0];
            Assert.True(ThemePalette.Contrast(fg, bg) >= 4.5, $"{dictionary} {level}: texte {ThemePalette.Contrast(fg, bg):0.00}");
            Assert.True(ThemePalette.Contrast(bg, surface) >= 1.25, $"{dictionary} {level}: fond du badge {ThemePalette.Contrast(bg, surface):0.00} sur la carte");
        }
    }

    [Fact]
    public void ChoosingMulticolore_IsSaved()
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings);
        vm.ThemeIndex = 3;
        settings.Received().Theme = AppTheme.Colorful;
    }

    private static XElement ThemeDictionary(string key)
    {
        string? dir = AppContext.BaseDirectory;
        while (dir is not null && !File.Exists(Path.Combine(dir, "app", "CleanSlate.App", "App.xaml")))
        {
            dir = Path.GetDirectoryName(dir);
        }

        return XDocument.Load(Path.Combine(dir!, "app", "CleanSlate.App", "App.xaml"))
            .Descendants(Xaml + "ResourceDictionary")
            .Single(d => (string?)d.Attribute(X + "Key") == key);
    }

    private static string[] ColorOf(XElement dictionary, string key)
    {
        XElement e = dictionary.Elements().Single(e => (string?)e.Attribute(X + "Key") == key);
        return e.Name.LocalName switch
        {
            "Color" => [e.Value.Trim()],
            "SolidColorBrush" => [(string)e.Attribute("Color")!],
            _ => e.Descendants(Xaml + "GradientStop").Select(s => (string)s.Attribute("Color")!).ToArray(),
        };
    }

    private static (double R, double G, double B) Rgb(string hex) =>
        (int.Parse(hex.AsSpan(1, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture) / 255.0,
         int.Parse(hex.AsSpan(3, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture) / 255.0,
         int.Parse(hex.AsSpan(5, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture) / 255.0);

    private static double Saturation(string hex)
    {
        var (r, g, b) = Rgb(hex);
        double max = Math.Max(r, Math.Max(g, b));
        double min = Math.Min(r, Math.Min(g, b));
        return max == 0 ? 0 : (max - min) / max;
    }

    private static double Hue(string hex)
    {
        var (r, g, b) = Rgb(hex);
        double max = Math.Max(r, Math.Max(g, b));
        double d = max - Math.Min(r, Math.Min(g, b));
        if (d == 0)
        {
            return 0;
        }

        double h = max == r ? ((g - b) / d) % 6 : max == g ? ((b - r) / d) + 2 : ((r - g) / d) + 4;
        h *= 60;
        return h < 0 ? h + 360 : h;
    }
}
