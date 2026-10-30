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

    /// <summary>Text <paramref name="key"/> with <c>{0}</c> filled.</summary>
    public static string One(string key, object? first) =>
        string.Format(French, Loader.GetString(key), first);

    /// <summary>Text <paramref name="key"/> with <c>{0}</c> and <c>{1}</c> filled.</summary>
    public static string Two(string key, object? first, object? second) =>
        string.Format(French, Loader.GetString(key), first, second);
}
