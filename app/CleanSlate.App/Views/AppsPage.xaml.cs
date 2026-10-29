using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Installed applications (EF-01 to EF-03).</summary>
public sealed partial class AppsPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public AppsPage()
    {
        ViewModel = App.GetService<AppsViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public AppsViewModel ViewModel { get; }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        await ViewModel.LoadCommand.ExecuteAsync(null);
        if (AppsList.Items.Count > 0)
        {
            AppsList.ScrollIntoView(AppsList.Items[0]);
        }
    }

    // SelectedItem is typed object: the view pushes the selection back to the ViewModel.
    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e) =>
        ViewModel.SelectedApp = AppsList.SelectedItem as AppItemViewModel;
}
