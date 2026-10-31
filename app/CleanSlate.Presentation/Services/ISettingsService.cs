namespace CleanSlate.Presentation.Services;

/// <summary>Theme of the application (ENF-08).</summary>
public enum AppTheme
{
    /// <summary>Follows Windows.</summary>
    System = 0,

    /// <summary>Always light.</summary>
    Light = 1,

    /// <summary>Always dark.</summary>
    Dark = 2,
}

/// <summary>User settings (Réglages page).</summary>
public interface ISettingsService
{
    /// <summary>
    /// « Supprimer définitivement, sans quarantaine » (D66): what is removed still goes through
    /// the quarantine, then is deleted at once; nothing can be restored. Off by default.
    /// </summary>
    bool DeleteImmediately { get; set; }

    /// <summary>Days before the quarantine is purged (EF-19), 1 to 365, 30 by default.</summary>
    int QuarantineDays { get; set; }

    /// <summary>The first-launch screens were completed (D63).</summary>
    bool FirstRunDone { get; set; }

    /// <summary>Data folder chosen for the quarantine and its history; null = <c>%LOCALAPPDATA%\CleanSlate</c> (D63).</summary>
    string? QuarantineFolder { get; set; }

    /// <summary>Folder of the exported logs; null = the Desktop (D63).</summary>
    string? ExportFolder { get; set; }

    /// <summary>Theme, following Windows by default.</summary>
    AppTheme Theme { get; set; }

    /// <summary>Folders or registry keys never proposed (EF-20).</summary>
    IReadOnlyList<string> Exclusions { get; }

    /// <summary>Adds an exclusion (ignored if already present).</summary>
    void AddExclusion(string location);

    /// <summary>Removes an exclusion.</summary>
    void RemoveExclusion(string location);

    /// <summary>Folder of the log files (ENF-10).</summary>
    string LogsFolder { get; }

    /// <summary>File holding these settings (exported with the logs).</summary>
    string SettingsFile { get; }

    /// <summary>Raised after a setting changed.</summary>
    event EventHandler? Changed;
}
