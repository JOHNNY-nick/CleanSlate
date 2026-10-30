using System.Globalization;
using System.Text;

namespace CleanSlate.Presentation;

/// <summary>
/// Log of the unexpected errors of the interface: <c>crash.yyyyMMdd.log</c> next to the
/// engine's logs (<c>%LOCALAPPDATA%\CleanSlate\logs</c>, ENF-10), or in the folder named
/// by <c>CLEANSLATE_LOG_DIR</c> (the Sandbox tests write it to their shared results, D56).
/// Each entry holds the full exception: type, message, stack trace, inner exceptions.
/// </summary>
public static class CrashLog
{
    /// <summary>Environment variable that moves the log folder (absolute path only).</summary>
    public const string DirectoryVariable = "CLEANSLATE_LOG_DIR";

    /// <summary>Log folder: <paramref name="overrideDirectory"/> when it is an absolute path, else the default.</summary>
    public static string LogDirectory(string? overrideDirectory, string localAppData) =>
        !string.IsNullOrWhiteSpace(overrideDirectory) && Path.IsPathFullyQualified(overrideDirectory)
            ? overrideDirectory
            : Path.Combine(localAppData, "CleanSlate", "logs");

    /// <summary>Log folder of this process.</summary>
    public static string CurrentDirectory() => LogDirectory(
        Environment.GetEnvironmentVariable(DirectoryVariable),
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData));

    /// <summary>One log entry.</summary>
    public static string Format(Exception exception, string source, DateTimeOffset when, string? detail = null)
    {
        var text = new StringBuilder();
        text.AppendLine(CultureInfo.InvariantCulture, $"==== {when:yyyy-MM-dd HH:mm:ss.fff zzz} — {source}");
        text.AppendLine(CultureInfo.InvariantCulture, $"Process {Environment.ProcessId}, .NET {Environment.Version}, {Environment.OSVersion}");
        if (!string.IsNullOrWhiteSpace(detail))
        {
            text.AppendLine(detail);
        }

        // ToString: type, message, stack trace and every inner exception.
        text.AppendLine(exception.ToString());
        text.AppendLine();
        return text.ToString();
    }

    /// <summary>
    /// Appends an entry to <c>crash.yyyyMMdd.log</c> in <paramref name="directory"/> and returns
    /// the file path, or null when it cannot be written. Never throws: it runs in error handlers.
    /// </summary>
    public static string? Write(string directory, Exception exception, string source, DateTimeOffset when, string? detail = null)
    {
        try
        {
            Directory.CreateDirectory(directory);
            string path = Path.Combine(directory, $"crash.{when:yyyyMMdd}.log");
            File.AppendAllText(path, Format(exception, source, when, detail), Encoding.UTF8);
            return path;
        }
        catch (Exception)
        {
            return null;
        }
    }
}
