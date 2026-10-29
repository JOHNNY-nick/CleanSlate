using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;

namespace CleanSlate.App.Services;

/// <summary><see cref="INavigationService"/> over the frame of <see cref="MainWindow"/>.</summary>
public sealed class NavigationService : INavigationService
{
    private readonly MainWindow _window;

    /// <summary>Creates the service for the main window.</summary>
    public NavigationService(MainWindow window)
    {
        _window = window;
    }

    /// <inheritdoc/>
    public void OpenUninstallWizard(InstalledApp app, bool complete)
    {
        App.GetService<UninstallWizardViewModel>().Start(app, complete);
        _window.ShowUninstallWizard();
    }

    /// <inheritdoc/>
    public void OpenHistory() => _window.ShowHistory();
}
