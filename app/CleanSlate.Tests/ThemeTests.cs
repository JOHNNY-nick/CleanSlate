using System.Xml.Linq;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Themes Clair, Sombre and Multicolore, the last one from the book's colors (D71).</summary>
public sealed class ThemeTests
{
    // Colors live in App.xaml (CLAUDE.md: no hard-coded color); read from there.
    private static readonly IReadOnlyDictionary<string, string> Colorful = ReadPalette();

    private static IReadOnlyDictionary<string, string> ReadPalette()
    {
        string? dir = AppContext.BaseDirectory;
        while (dir is not null && !File.Exists(Path.Combine(dir, "app", "CleanSlate.App", "App.xaml")))
        {
            dir = Path.GetDirectoryName(dir);
        }

        XNamespace xaml = "http://schemas.microsoft.com/winfx/2006/xaml/presentation";
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        var doc = XDocument.Load(Path.Combine(dir!, "app", "CleanSlate.App", "App.xaml"));
        return doc.Descendants(xaml + "ResourceDictionary")
            .Single(d => (string?)d.Attribute(x + "Key") == ThemePalette.ColorfulKey)
            .Elements(xaml + "Color")
            .ToDictionary(c => (string)c.Attribute(x + "Key")!, c => c.Value.Trim());
    }

    [Fact]
    public void TheColorfulPalette_UsesTheBookColors_AndCoversEveryToken()
    {
        var p = Colorful;
        Assert.Equal("#6C4DFF", p["AccentBrush"]);
        Assert.Equal("#1B1740", p["TextPrimaryBrush"]);
        Assert.Equal("#EAE5FF", p["WindowBackgroundBrush"]);
        Assert.Equal("#C81D25", p["DangerBrush"]);
        Assert.All(ThemePalette.Tokens, t => Assert.True(p.ContainsKey(t), t));
    }

    [Theory]
    [InlineData("TextPrimaryBrush", "SurfaceBrush", 7.0)]
    [InlineData("TextPrimaryBrush", "WindowBackgroundBrush", 7.0)]
    [InlineData("TextSecondaryBrush", "SurfaceBrush", 4.5)]
    [InlineData("TextSecondaryBrush", "WindowBackgroundBrush", 4.5)]
    [InlineData("OnAccentBrush", "AccentButtonBackground", 4.5)]
    [InlineData("DangerBrush", "SurfaceBrush", 4.5)]
    [InlineData("ConfidenceHighForegroundBrush", "ConfidenceHighBackgroundBrush", 4.5)]
    [InlineData("ConfidenceMediumForegroundBrush", "ConfidenceMediumBackgroundBrush", 4.5)]
    [InlineData("ConfidenceLowForegroundBrush", "ConfidenceLowBackgroundBrush", 4.5)]
    public void EveryTextOfTheColorfulTheme_StaysReadable(string text, string background, double minimum) =>
        Assert.True(
            ThemePalette.Contrast(Colorful[text], Colorful[background]) >= minimum,
            $"{text} on {background}: {ThemePalette.Contrast(Colorful[text], Colorful[background]):0.00}");

    [Fact]
    public void Contrast_FollowsTheWcagFormula()
    {
        Assert.Equal(21.0, ThemePalette.Contrast("#000000", "#FFFFFF"), 2);
        Assert.Equal(1.0, ThemePalette.Contrast("#6C4DFF", "#6C4DFF"), 2);
    }

    [Theory]
    [InlineData(AppTheme.Light, 0)]
    [InlineData(AppTheme.Dark, 1)]
    [InlineData(AppTheme.Colorful, 2)]
    public void TheSettings_OfferClairSombreMulticolore(AppTheme theme, int index)
    {
        Assert.Equal(index, ThemePalette.IndexOf(theme));
        Assert.Equal(theme, ThemePalette.FromIndex(index));
    }

    [Fact]
    public void FollowingWindows_BecomesTheThemeWindowsShowsNow()
    {
        Assert.Equal(AppTheme.Dark, ThemePalette.Resolve(AppTheme.System, windowsIsDark: true));
        Assert.Equal(AppTheme.Light, ThemePalette.Resolve(AppTheme.System, windowsIsDark: false));
        Assert.Equal(AppTheme.Colorful, ThemePalette.Resolve(AppTheme.Colorful, windowsIsDark: true));
    }

    [Fact]
    public void ChoosingMulticolore_IsSaved()
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings);
        vm.ThemeIndex = 2;
        settings.Received().Theme = AppTheme.Colorful;
    }
}
