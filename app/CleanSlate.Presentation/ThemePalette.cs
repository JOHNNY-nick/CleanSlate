using System.Globalization;
using CleanSlate.Presentation.Services;

namespace CleanSlate.Presentation;

/// <summary>
/// Themes of the Réglages page (D71, D72): Clair, Sombre and Multicolore. Multicolore (App.xaml,
/// <see cref="ColorfulKey"/>) puts colors everywhere on the light base: a red-yellow-green-blue-
/// violet window, a rainbow band on top of every card, gradient titles and main buttons, one
/// color per menu icon, vivid confidence badges. Every text keeps a WCAG contrast of at least
/// 4.5:1 on each color of its background (7:1 for the main text, 3:1 for the large titles).
/// </summary>
public static class ThemePalette
{
    /// <summary>Tokens of App.xaml (colors, gradients, the card outline) changed by Multicolore.</summary>
    public static readonly string[] Tokens =
    [
        "AccentBrush", "AccentHoverBrush", "AccentSubtleBrush", "OnAccentBrush",
        "WindowBackgroundBrush", "SurfaceBrush", "BorderBrush", "ControlBorderBrush",
        "TextPrimaryBrush", "TextSecondaryBrush", "DangerBrush",
        "ConfidenceHighForegroundBrush", "ConfidenceHighBackgroundBrush",
        "ConfidenceMediumForegroundBrush", "ConfidenceMediumBackgroundBrush",
        "ConfidenceLowForegroundBrush", "ConfidenceLowBackgroundBrush",
        "AccentButtonBackground", "AccentButtonBackgroundPointerOver", "AccentButtonBackgroundPressed",
        "AccentButtonForeground", "AccentButtonForegroundPointerOver", "AccentButtonForegroundPressed",
        "AccentFillColorDefaultBrush", "AccentFillColorSecondaryBrush", "AccentFillColorTertiaryBrush",
        "AccentTextFillColorPrimaryBrush", "AccentTextFillColorSecondaryBrush", "AccentTextFillColorTertiaryBrush",
        "TitleBrush", "CardBorderThickness",
        "NavHomeBrush", "NavAppsBrush", "NavOrphansBrush", "NavHistoryBrush", "NavStorageBrush", "NavSettingsBrush",
        "PageHeaderForegroundBrush", "PageHeaderStripBrush", "ProgressBarForeground", "ContentSurfaceBrush", "PaneBackgroundBrush", "OnDangerBrush",
        "PageHeaderHomeBrush", "PageHeaderAppsBrush", "PageHeaderOrphansBrush", "PageHeaderHistoryBrush", "PageHeaderStorageBrush", "PageHeaderSettingsBrush", "PageHeaderWizardBrush",
    ];

    /// <summary>
    /// Theme dictionary of App.xaml holding the tokens shown for <paramref name="theme"/>
    /// (Multicolore recolors the light one; Système follows Windows). Badges must be looked up
    /// there: Application.Resources answers with Windows' theme, whatever was chosen (D72).
    /// </summary>
    public static string DictionaryKey(AppTheme theme, bool windowsIsDark) => theme switch
    {
        AppTheme.Dark => "Dark",
        AppTheme.System => windowsIsDark ? "Dark" : "Light",
        _ => "Light",
    };

    /// <summary>Key of the Multicolore palette in App.xaml (one <c>Color</c> per token).</summary>
    public const string ColorfulKey = "ColorfulPalette";

    /// <summary>Position in the Réglages radio buttons: Système, Clair, Sombre, Multicolore (D72).</summary>
    public static int IndexOf(AppTheme theme) => Enum.IsDefined(theme) ? (int)theme : 0;

    /// <summary>Theme of a radio button position.</summary>
    public static AppTheme FromIndex(int index) => Enum.IsDefined(typeof(AppTheme), index) ? (AppTheme)index : AppTheme.System;
    /// <summary>WCAG 2 contrast ratio of two « #RRGGBB » colors, 1 to 21.</summary>
    public static double Contrast(string first, string second)
    {
        double a = Luminance(first);
        double b = Luminance(second);
        return (Math.Max(a, b) + 0.05) / (Math.Min(a, b) + 0.05);
    }

    private static double Luminance(string hex)
    {
        double Channel(int start)
        {
            double c = int.Parse(hex.AsSpan(start, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture) / 255.0;
            return c <= 0.03928 ? c / 12.92 : Math.Pow((c + 0.055) / 1.055, 2.4);
        }

        return (0.2126 * Channel(1)) + (0.7152 * Channel(3)) + (0.0722 * Channel(5));
    }
}
