using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;

namespace CleanSlate.App.Services;

/// <summary><see cref="INavigationService"/> over the frame of <see cref="MainWindow"/>.</summary>
public sealed class NavigationService : INavigationService
{
    // Resolved on use: MainWindow builds the first page (and its ViewModels)
    // in its constructor, so taking it here would be a dependency cycle.
    private static MainWindow Window => App.GetService<MainWindow>();

    /// <inheritdoc/>
    public void OpenUninstallWizard(InstalledApp app, bool complete)
    {
        App.GetService<UninstallWizardViewModel>().Start(app, complete);
        Window.ShowUninstallWizard();
    }

    /// <inheritdoc/>
    public void OpenHistory() => Window.ShowPage("History");

    /// <inheritdoc/>
    public void OpenHome() => Window.ShowPage("Home");

    /// <inheritdoc/>
    public void OpenApps() => Window.ShowPage("Apps");

    /// <inheritdoc/>
    public void OpenStorage() => Window.ShowPage("Storage");

    /// <inheritdoc/>
    public void OpenManager() => Window.ShowPage("Manager");

    /// <inheritdoc/>
    public void OpenSettings() => Window.ShowPage("Settings");

    /// <inheritdoc/>
    public void OpenOrphans(bool startScan)
    {
        Window.ShowPage("Orphans");
        OrphansViewModel orphans = App.GetService<OrphansViewModel>();
        if (startScan && !orphans.IsScanning)
        {
            orphans.ScanCommand.Execute(null);
        }
    }
}
