using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>History of cleaning sessions (EF-18 to EF-20).</summary>
public sealed partial class HistoryPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public HistoryPage()
    {
        ViewModel = App.GetService<HistoryViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public HistoryViewModel ViewModel { get; }

    private void OnLoaded(object sender, RoutedEventArgs e) => ViewModel.LoadCommand.Execute(null);

    // The restore button lives in a session template: it hands its session to the command.
    private void OnRestoreClick(object sender, RoutedEventArgs e) =>
        ViewModel.RestoreCommand.Execute((sender as FrameworkElement)?.Tag as SessionViewModel);
}
