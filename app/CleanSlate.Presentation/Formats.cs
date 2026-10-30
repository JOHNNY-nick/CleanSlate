using System.Globalization;

namespace CleanSlate.Presentation;

/// <summary>
/// French display formats of design § 5: sizes in Ko / Mo / Go with a decimal
/// comma, dates such as « 22 oct. 2026 ».
/// </summary>
public static class Formats
{
    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");
    private static readonly string[] Units = ["Ko", "Mo", "Go", "To"];

    /// <summary>Placeholder shown for an unknown value.</summary>
    public const string Unknown = "—";

    /// <summary>
    /// Formats a size: « 5,6 Mo », « 412 Mo », « 1,1 Go ». One decimal below 10,
    /// none above; binary multiples (1 Ko = 1 024 octets), as Windows does.
    /// </summary>
    public static string Size(ulong bytes)
    {
        if (bytes < 1024)
        {
            return string.Create(French, $"{bytes} octets");
        }

        double value = bytes / 1024.0;
        int unit = 0;
        while (value >= 1024 && unit < Units.Length - 1)
        {
            value /= 1024;
            unit++;
        }

        string number = value < 10
            ? Math.Round(value, 1, MidpointRounding.AwayFromZero).ToString("0.#", French)
            : Math.Round(value, 0, MidpointRounding.AwayFromZero).ToString("0", French);
        return $"{number} {Units[unit]}";
    }

    /// <summary>Formats a size given in KiB, or « — » when unknown.</summary>
    public static string SizeFromKb(ulong? kilobytes) =>
        kilobytes is { } kb ? Size(kb * 1024) : Unknown;

    /// <summary>Formats a date: « 22 oct. 2026 ».</summary>
    public static string Date(DateOnly date) => date.ToString("d MMM yyyy", French);

    /// <summary>Whole percentage, e.g. « 43 % » for 42.6.</summary>
    public static string Percent(double percent) => string.Format(French, "{0:0} %", percent);

    /// <summary>Formats an optional date, or « — » when unknown.</summary>
    public static string Date(DateOnly? date) => date is { } d ? Date(d) : Unknown;
}
