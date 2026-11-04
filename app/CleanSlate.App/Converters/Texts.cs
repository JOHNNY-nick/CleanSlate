using System.Globalization;
using Microsoft.Windows.ApplicationModel.Resources;

namespace CleanSlate.App.Converters;

/// <summary>
/// French sentences with numbers, for x:Bind function bindings: the text of
/// Resources.resw holds <c>{0}</c> and <c>{1}</c>, e.g. <c>Format_Simulation</c>.
/// One sentence per InfoBar message, instead of runs glued together.
/// </summary>
public static class Texts
{
    private static readonly ResourceLoader Loader = new();
    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");

    /// <summary>Text <paramref name="key"/> of Resources.resw, for a view built in code.</summary>
    public static string Get(string key) => Loader.GetString(key);

    /// <summary>Text <paramref name="key"/> with <c>{0}</c> filled.</summary>
    public static string One(string key, object? first) =>
        string.Format(French, Loader.GetString(key), first);

    /// <summary>Text <paramref name="key"/> with <c>{0}</c> and <c>{1}</c> filled.</summary>
    public static string Two(string key, object? first, object? second) =>
        string.Format(French, Loader.GetString(key), first, second);

    /// <summary>Visible when <paramref name="text"/> is not empty.</summary>
    public static Microsoft.UI.Xaml.Visibility IsSet(string? text) =>
        string.IsNullOrEmpty(text) ? Microsoft.UI.Xaml.Visibility.Collapsed : Microsoft.UI.Xaml.Visibility.Visible;

    /// <summary>True when <paramref name="text"/> is not empty (InfoBar.IsOpen).</summary>
    public static bool HasText(string? text) => !string.IsNullOrEmpty(text);
}
