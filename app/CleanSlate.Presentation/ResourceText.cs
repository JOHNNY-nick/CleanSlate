namespace CleanSlate.Presentation;

/// <summary>
/// French text of an enum value, read from Resources.resw under the key
/// <c>{prefix}_{value}</c> (e.g. <c>Kind_Folder</c>). Used by the view's ResourceStringConverter.
/// </summary>
public static class ResourceText
{
    /// <summary>
    /// Looks up <c>{prefix}_{value}</c> with <paramref name="getString"/>. Never throws: an empty
    /// value is never looked up (empty text), and a missing key gives the value's name. The
    /// resource loader of the Windows App SDK throws for a missing key, and an exception in a
    /// converter closes the application without a message (crash of 30 Oct. 2026, D56).
    /// </summary>
    public static string Resolve(Func<string, string> getString, object? prefix, object? value)
    {
        string name = value?.ToString() ?? string.Empty;
        if (name.Length == 0)
        {
            return string.Empty;
        }

        try
        {
            string text = getString($"{prefix}_{name}");
            return string.IsNullOrEmpty(text) ? name : text;
        }
        catch (Exception)
        {
            return name;
        }
    }
}
