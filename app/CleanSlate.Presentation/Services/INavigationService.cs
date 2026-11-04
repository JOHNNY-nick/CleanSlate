using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>Navigation between pages, driven by the ViewModels.</summary>
public interface INavigationService
{
    /// <summary>
    /// Opens the uninstall wizard for <paramref name="app"/>: full uninstall
    /// with leftovers (EF-12, EF-13) or, if <paramref name="complete"/> is
    /// false, the official uninstaller only (« Désinstallation classique »).
    /// </summary>
    void OpenUninstallWizard(InstalledApp app, bool complete);

    /// <summary>Opens the History page.</summary>
    void OpenHistory();

    /// <summary>Opens the Home page.</summary>
    void OpenHome();

    /// <summary>Opens the Settings page (simulation switch, design § 5).</summary>
    void OpenSettings();

    /// <summary>Opens the Orphans page, and starts a scan if asked.</summary>
    void OpenOrphans(bool startScan);

    /// <summary>Opens the Applications page (from the storage map, D73).</summary>
    void OpenApps();

    /// <summary>Opens the Stockage page (from the Home preview, D73).</summary>
    void OpenStorage();

    /// <summary>Opens the Gestionnaire (D79).</summary>
    void OpenManager();

    /// <summary>
    /// Opens the Gestionnaire on the files of <paramref name="folder"/> (from the storage map,
    /// D81): it analyzes that folder and shows « Gros fichiers ».
    /// </summary>
    void OpenManager(string folder);

    /// <summary>Folder handed by <see cref="OpenManager(string)"/>, once; null otherwise.</summary>
    string? TakeManagerFolder();
}
