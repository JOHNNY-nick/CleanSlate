using System.Globalization;
using System.Text;

namespace CleanSlate.Presentation;

/// <summary>
/// Log of the unexpected errors of the interface: <c>crash.yyyyMMdd.log</c> next to the
/// engine's logs (<c>%LOCALAPPDATA%\CleanSlate\logs</c>, ENF-10), or in the folder named
/// by the value <c>LogDirectory</c> of <c>HKLM\SOFTWARE\CleanSlate</c>, which only an
/// administrator can write (D93; the Sandbox tests point it at their shared results, D56).
/// Each entry holds the full exception: type, message, stack trace, inner exceptions.
/// </summary>
public static class CrashLog
{
    /// <summary>Key of the machine that holds <see cref="DirectoryValue"/> (as the engine).</summary>
    public const string SettingsKey = @"SOFTWARE\CleanSlate";

    /// <summary>Value that moves the log folder (absolute path only).</summary>
    public const string DirectoryValue = "LogDirectory";

    /// <summary>Log folder: <paramref name="overrideDirectory"/> when it is an absolute path, else the default.</summary>
    public static string LogDirectory(string? overrideDirectory, string localAppData) =>
        !string.IsNullOrWhiteSpace(overrideDirectory) && Path.IsPathFullyQualified(overrideDirectory)
            ? overrideDirectory
            : Path.Combine(localAppData, "CleanSlate", "logs");

    /// <summary>Log folder of this process.</summary>
    public static string CurrentDirectory() => LogDirectory(
        MachineDirectory(),
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData));

    // The value of the machine, read-only; null when absent or unreadable.
    private static string? MachineDirectory()
    {
        if (!OperatingSystem.IsWindows())
        {
            return null;
        }

        try
        {
            using var machine = Microsoft.Win32.RegistryKey.OpenBaseKey(Microsoft.Win32.RegistryHive.LocalMachine, Microsoft.Win32.RegistryView.Registry64);
            using var key = machine.OpenSubKey(SettingsKey);
            return key?.GetValue(DirectoryValue) as string;
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException or IOException)
        {
            return null;
        }
    }

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
