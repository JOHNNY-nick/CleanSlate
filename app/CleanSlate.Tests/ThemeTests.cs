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

    [Theory]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
    public void EveryTokenOfTheTheme_IsInThePalette(string palette)
    {
        XElement d = ThemeDictionary(palette);
        Assert.All(ThemePalette.Tokens, t => Assert.NotNull(d.Elements().SingleOrDefault(e => (string?)e.Attribute(X + "Key") == t)));
        // D76: a plain wall, no rainbow edging, no colored band under the titles.
        Assert.Equal("Color", d.Elements().Single(e => (string?)e.Attribute(X + "Key") == "WindowBackgroundBrush").Name.LocalName);
        Assert.Equal("3", d.Elements().Single(e => (string?)e.Attribute(X + "Key") == "CardBorderThickness").Value.Trim()); // ink outline
        Assert.StartsWith("#00", ColorOf(d, "PageHeaderStripBrush")[0], StringComparison.Ordinal);
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

    /// <summary>The eight paint colors asked by the owner (D76), vivid in both palettes.</summary>
    [Theory]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
    public void Multicolore_PaintsInEveryVividColor(string palette)
    {
        XElement d = ThemeDictionary(palette);
        double[] hues = Enumerable.Range(0, PaintComposer.ColorCount)
            .Select(i => ColorOf(d, $"Paint{i}")[0])
            .Where(c => Saturation(c) >= 0.55)
            .Select(Hue)
            .ToArray();
        Assert.Equal(PaintComposer.ColorCount, hues.Length);
        (string Name, double From, double To)[] families =
            [("rouge", 350, 365), ("orange", 25, 40), ("jaune", 45, 60), ("vert", 100, 140), ("bleu", 200, 215), ("violet", 265, 295), ("rose", 325, 345), ("turquoise", 170, 195)];
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

    /// <summary>
    /// D76: cards, lists and loose text sit on a slightly translucent surface; laid over any
    /// paint color, the text on it stays readable (at least 4.5:1).
    /// </summary>
    [Theory]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
    public void TextOnTheSurface_StaysReadable_OverEveryPaint(string palette)
    {
        XElement d = ThemeDictionary(palette);
        string surface = ColorOf(d, "PaintSurfaceColor")[0];
        IEnumerable<string> below = Enumerable.Range(0, PaintComposer.ColorCount).Select(i => ColorOf(d, $"Paint{i}")[0]).Append(ColorOf(d, "WindowBackgroundBrush")[0]);
        foreach (string paint in below)
        {
            string seen = ThemePalette.Over(surface, paint);
            foreach (string text in new[] { "TextPrimaryBrush", "TextSecondaryBrush" })
            {
                double ratio = ThemePalette.Contrast(ColorOf(d, text)[0], seen);
                Assert.True(ratio >= 4.5, $"{palette} {text} over {paint}: {ratio:0.00}");
            }
        }
    }

    /// <summary>
    /// D76, street style: titles in ink on a paper sticker; menu icons in ink on a drop of the
    /// page color (an icon: 3:1); the active item in green with its text readable.
    /// </summary>
    [Theory]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
    public void TitlesIconsAndTheActiveItem_StayReadable(string palette)
    {
        XElement d = ThemeDictionary(palette);
        string paper = ColorOf(d, "PaintSurfaceColor")[0][3..];
        Assert.True(ThemePalette.Contrast(ColorOf(d, "PageHeaderForegroundBrush")[0], "#" + paper) >= 4.5, $"{palette} title on its sticker");
        foreach (string page in new[] { "Home", "Apps", "Orphans", "History", "Storage", "Settings" })
        {
            double icon = ThemePalette.Contrast(ColorOf(d, $"Nav{page}Brush")[0], ColorOf(d, $"PageHeader{page}Brush")[0]);
            Assert.True(icon >= 3.0, $"{palette} icon {page} on its drop: {icon:0.00}");
        }

        Assert.True(ThemePalette.Contrast(ColorOf(d, "NavActiveTextColor")[0], ColorOf(d, "NavActiveColor")[0]) >= 4.5, $"{palette} active item");
        Assert.True(ThemePalette.Contrast(ColorOf(d, "AccentButtonForeground")[0], ColorOf(d, "AccentButtonBackground")[0]) >= 4.5, $"{palette} main button");
        foreach (string state in new[] { "Completed", "Partial", "Restored", "Purged" })
        {
            Assert.True(ThemePalette.Contrast(ColorOf(d, "Ink")[0], ColorOf(d, $"Status{state}Color")[0]) >= 4.5, $"{palette} state {state}");
        }

        string label = ThemePalette.Over(ColorOf(d, "LabelStickerColor")[0], ColorOf(d, "StorageQuarantineBrush")[0]);
        Assert.True(ThemePalette.Contrast(ColorOf(d, "StorageTileTextBrush")[0], label) >= 4.5, $"{palette} map label");
    }

    [Fact]
    public void Over_And_Mix_BlendColors()
    {
        Assert.Equal("#FFFFFF", ThemePalette.Over("#FFFFFFFF", "#000000"));
        Assert.Equal("#000000", ThemePalette.Over("#00FFFFFF", "#000000"));
        Assert.Equal("#808080", ThemePalette.Mix("#000000", "#FFFFFF", 0.502));
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

    [Fact]
    public void MulticoloreOnDark_UsesTheDarkDictionary()
    {
        Assert.Equal("Dark", ThemePalette.DictionaryKey(AppTheme.Colorful, windowsIsDark: false, paintDark: true));
        Assert.Equal(ThemePalette.ColorfulDarkKey, ThemePalette.PaletteKey(paintDark: true));
        Assert.Equal(ThemePalette.ColorfulKey, ThemePalette.PaletteKey(paintDark: false));
    }

    /// <summary>Each badge shows its text readably and stands out from the card it sits on (D72).</summary>
    [Theory]
    [InlineData("Light")]
    [InlineData("Dark")]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
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

    /// <summary>Names written in the rectangles of the storage map stay readable on each category (D73).</summary>
    [Theory]
    [InlineData("Light")]
    [InlineData("Dark")]
    [InlineData(ThemePalette.ColorfulKey)]
    [InlineData(ThemePalette.ColorfulDarkKey)]
    public void StorageMapLabels_AreReadableOnEveryCategory(string dictionary)
    {
        XElement d = ThemeDictionary(dictionary);
        string text = ColorOf(d, "StorageTileTextBrush")[0];
        foreach (string category in new[] { "Applications", "Leftovers", "Quarantine", "UserFiles", "System", "Other" })
        {
            string fill = ColorOf(d, $"Storage{category}Brush")[0];
            Assert.True(ThemePalette.Contrast(text, fill) >= 4.5, $"{dictionary} {category}: {ThemePalette.Contrast(text, fill):0.00}");
        }
    }

    [Fact]
    public void ChoosingMulticolore_IsSaved()
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings);
        Assert.False(vm.IsColorful);
        vm.ThemeIndex = 3;
        settings.Received().Theme = AppTheme.Colorful;
        Assert.True(vm.IsColorful); // its three styles and the dark background are offered
    }

    [Theory]
    [InlineData(0, PaintStyle.Splashes)]
    [InlineData(1, PaintStyle.Watercolor)]
    [InlineData(2, PaintStyle.Explosion)]
    public void ThePaintStyle_AndTheDarkBackground_AreSaved(int index, PaintStyle style)
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings) { PaintStyleIndex = index == 0 ? 2 : 0 };
        vm.PaintStyleIndex = index;
        vm.PaintDark = true;
        settings.Received().PaintStyle = style;
        settings.Received().PaintDark = true;
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
