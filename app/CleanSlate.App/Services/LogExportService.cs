using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using Microsoft.Win32;

namespace CleanSlate.App.Services;

/// <summary>
/// Writes <c>CleanSlate-journaux-yyyyMMdd-HHmmss.zip</c> in the export folder of the settings,
/// else on the Desktop (known folder: also when it is redirected to OneDrive), with
/// <see cref="LogExport"/>. Nothing is sent.
/// </summary>
public sealed class LogExportService : ILogExportService
{
    private readonly ISettingsService _settings;
    private readonly IEngineService _engine;

    /// <summary>Creates the service.</summary>
    public LogExportService(ISettingsService settings, IEngineService engine)
    {
        _settings = settings;
        _engine = engine;
    }

    /// <inheritdoc/>
    public async Task<string> ExportAsync(CancellationToken cancellationToken)
    {
        string engineVersion;
        try
        {
            engineVersion = await _engine.GetVersionAsync(cancellationToken);
        }
        catch (Interop.EngineException)
        {
            engineVersion = "?";
        }

        string system = SystemText(engineVersion);
        // Folder chosen at the first launch or in Réglages (D63), else the Desktop.
        string folder = string.IsNullOrWhiteSpace(_settings.ExportFolder) ? Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory) : _settings.ExportFolder;
        Directory.CreateDirectory(folder);
        string zip = Path.Combine(folder, $"CleanSlate-journaux-{DateTime.Now:yyyyMMdd-HHmmss}.zip");
        var replacements = LogExport.Replacements(
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            Environment.UserName,
            Environment.MachineName);
        await Task.Run(() => LogExport.Create(zip, _settings.LogsFolder, _settings.SettingsFile, system, replacements), cancellationToken);
        return zip;
    }

    /// <inheritdoc/>
    public void Reveal(string path) =>
        Launcher.Explorer($"/select,{path}");

    private string SystemText(string engineVersion)
    {
        string app = typeof(LogExportService).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion ?? "?";
        string displayVersion = "?";
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
            displayVersion = $"{key?.GetValue("DisplayVersion")} (build {key?.GetValue("CurrentBuild")}.{key?.GetValue("UBR")})";
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException or IOException)
        {
            // The version line stays « ? ».
        }

        var text = new StringBuilder();
        text.AppendLine(CultureInfo.InvariantCulture, $"Export : {DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss zzz}");
        text.AppendLine(CultureInfo.InvariantCulture, $"CleanSlate : {app}");
        text.AppendLine(CultureInfo.InvariantCulture, $"Moteur : {engineVersion}");
        text.AppendLine(CultureInfo.InvariantCulture, $"Windows : {RuntimeInformation.OSDescription}, {displayVersion}, {RuntimeInformation.OSArchitecture}");
        text.AppendLine(CultureInfo.InvariantCulture, $"Langue : {CultureInfo.CurrentUICulture.Name}");
        text.AppendLine(CultureInfo.InvariantCulture, $".NET : {Environment.Version}");
        text.AppendLine(CultureInfo.InvariantCulture, $"Quarantaine : {(_settings.DeleteImmediately ? "désactivée (suppression immédiate)" : $"{_settings.QuarantineDays} jours")} ; exclusions : {_settings.Exclusions.Count}");
        return text.ToString();
    }
}
