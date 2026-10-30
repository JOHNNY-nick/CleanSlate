using System.IO.Compression;
using System.Text;
using System.Text.RegularExpressions;

namespace CleanSlate.Presentation;

/// <summary>
/// Zip of the logs and the settings, to attach to a problem report (ENF-10). Personal parts
/// are replaced before anything is written: the profile folder becomes <c>%USERPROFILE%</c>,
/// the computer and account names become <c>&lt;ordinateur&gt;</c> and <c>&lt;utilisateur&gt;</c>.
/// Nothing is sent anywhere: the zip stays where the caller puts it.
/// </summary>
public static class LogExport
{
    /// <summary>Folder of the logs inside the zip.</summary>
    public const string LogsEntry = "journaux/";

    /// <summary>Settings inside the zip.</summary>
    public const string SettingsEntry = "configuration/settings.json";

    /// <summary>Versions and system inside the zip.</summary>
    public const string SystemEntry = "systeme.txt";

    /// <summary>
    /// Replacements for this account: the profile folder, « computer\account », then the
    /// computer name alone. The account name alone is not replaced: a short name (« HP »)
    /// would also hit ordinary words.
    /// </summary>
    public static IReadOnlyList<KeyValuePair<string, string>> Replacements(string userProfile, string userName, string machineName)
    {
        var list = new List<KeyValuePair<string, string>>();
        if (!string.IsNullOrWhiteSpace(userProfile))
        {
            list.Add(new(userProfile.TrimEnd('\\'), "%USERPROFILE%"));
        }

        if (!string.IsNullOrWhiteSpace(machineName) && !string.IsNullOrWhiteSpace(userName))
        {
            list.Add(new($@"{machineName}\{userName}", @"<ordinateur>\<utilisateur>"));
        }

        if (!string.IsNullOrWhiteSpace(machineName))
        {
            list.Add(new(machineName, "<ordinateur>"));
        }

        return list;
    }

    /// <summary>
    /// Replaces each value in <paramref name="text"/>, ignoring case, only as a whole name
    /// (« C:\Users\Ana » but not « C:\Users\Anabel »), written plainly or JSON-escaped
    /// (« C:\\Users\\Ana »). Longest values first.
    /// </summary>
    public static string Redact(string text, IReadOnlyList<KeyValuePair<string, string>> replacements)
    {
        foreach (var (value, placeholder) in replacements.OrderByDescending(r => r.Key.Length))
        {
            foreach (var (form, replacement) in new[] { (value, placeholder), (value.Replace(@"\", @"\\"), placeholder.Replace(@"\", @"\\")) }.Distinct())
            {
                text = Regex.Replace(
                    text,
                    $@"(?<![\w-]){Regex.Escape(form)}(?![\w-])",
                    replacement.Replace("$", "$$"),
                    RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
            }
        }

        return text;
    }

    /// <summary>
    /// Writes the zip at <paramref name="zipPath"/> (never overwritten): the <c>*.log</c> files
    /// at the top of <paramref name="logsDirectory"/> (links and junctions skipped), the
    /// settings file if it exists, and <paramref name="systemText"/>; every text redacted.
    /// Returns the number of log files.
    /// </summary>
    public static int Create(
        string zipPath,
        string logsDirectory,
        string? settingsFile,
        string systemText,
        IReadOnlyList<KeyValuePair<string, string>> replacements)
    {
        var logs = Directory.Exists(logsDirectory)
            ? new DirectoryInfo(logsDirectory).EnumerateFiles("*.log")
                .Where(f => !f.Attributes.HasFlag(FileAttributes.ReparsePoint))
                .OrderBy(f => f.Name, StringComparer.OrdinalIgnoreCase)
                .ToList()
            : [];

        using var stream = new FileStream(zipPath, FileMode.CreateNew, FileAccess.Write);
        using var zip = new ZipArchive(stream, ZipArchiveMode.Create);
        foreach (var log in logs)
        {
            AddText(zip, LogsEntry + log.Name, Redact(ReadShared(log.FullName), replacements));
        }

        if (settingsFile is not null && File.Exists(settingsFile)
            && !File.GetAttributes(settingsFile).HasFlag(FileAttributes.ReparsePoint))
        {
            AddText(zip, SettingsEntry, Redact(ReadShared(settingsFile), replacements));
        }

        AddText(zip, SystemEntry, Redact(systemText, replacements));
        return logs.Count;
    }

    // The engine keeps today's log open: read it without locking it.
    private static string ReadShared(string path)
    {
        using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        using var reader = new StreamReader(file, Encoding.UTF8);
        return reader.ReadToEnd();
    }

    private static void AddText(ZipArchive zip, string name, string text)
    {
        using var writer = new StreamWriter(zip.CreateEntry(name, CompressionLevel.Optimal).Open(), new UTF8Encoding(false));
        writer.Write(text);
    }
}
