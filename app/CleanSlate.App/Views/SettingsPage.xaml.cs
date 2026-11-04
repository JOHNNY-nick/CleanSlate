using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Settings page (EF-19, EF-20, ENF-08, ENF-10).</summary>
public sealed partial class SettingsPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public SettingsPage()
    {
        ViewModel = App.GetService<SettingsViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public SettingsViewModel ViewModel { get; }

    private void OnLoaded(object sender, RoutedEventArgs e) => ViewModel.LoadCommand.Execute(null);

    // The remove button lives in an item template: it hands its exclusion to the command.
    private void OnRemoveExclusion(object sender, RoutedEventArgs e) =>
        ViewModel.RemoveExclusionCommand.Execute((sender as FrameworkElement)?.Tag as string);

    // Opening a folder in Explorer is a view concern (no business logic).
    private void OnOpenLogs(object sender, RoutedEventArgs e)
    {
        Directory.CreateDirectory(ViewModel.LogsFolder);
        Services.Launcher.Explorer(ViewModel.LogsFolder);
    }
}
