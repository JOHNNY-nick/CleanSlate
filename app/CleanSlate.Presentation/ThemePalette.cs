using System.Globalization;
using CleanSlate.Presentation.Services;

namespace CleanSlate.Presentation;

/// <summary>
/// Themes of the Réglages page (D71): Clair, Sombre and Multicolore. Multicolore (App.xaml,
/// <see cref="ColorfulKey"/>) is the light
/// base recolored with the book's palette (<c>book/book.css</c>): violet accent, ink text,
/// lavender window, and the book's green, pink and plum for the confidence levels. Every text
/// keeps a WCAG contrast of at least 4.5:1 on its background (7:1 for the main text).
/// </summary>
public static class ThemePalette
{
    /// <summary>Color tokens of App.xaml recolored by the Multicolore theme.</summary>
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
    ];

    /// <summary>Key of the Multicolore palette in App.xaml (one <c>Color</c> per token).</summary>
    public const string ColorfulKey = "ColorfulPalette";

    /// <summary>Position in the Réglages radio buttons: Clair, Sombre, Multicolore.</summary>
    public static int IndexOf(AppTheme theme) => theme switch
    {
        AppTheme.Dark => 1,
        AppTheme.Colorful => 2,
        _ => 0,
    };

    /// <summary>Theme of a radio button position.</summary>
    public static AppTheme FromIndex(int index) => index switch
    {
        1 => AppTheme.Dark,
        2 => AppTheme.Colorful,
        _ => AppTheme.Light,
    };

    /// <summary>« Suivre Windows » (former choice) becomes what Windows shows now.</summary>
    public static AppTheme Resolve(AppTheme stored, bool windowsIsDark) =>
        stored == AppTheme.System ? (windowsIsDark ? AppTheme.Dark : AppTheme.Light) : stored;

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
