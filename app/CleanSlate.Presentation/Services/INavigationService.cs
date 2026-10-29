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

    /// <summary>Opens the Orphans page, and starts a scan if asked.</summary>
    void OpenOrphans(bool startScan);
}
