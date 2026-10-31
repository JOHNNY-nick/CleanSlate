using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>« Stockage » page (D69).</summary>
public sealed partial class StoragePage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public StoragePage()
    {
        ViewModel = App.GetService<StorageViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public StorageViewModel ViewModel { get; }

    private void OnLoaded(object sender, RoutedEventArgs e) => ViewModel.LoadCommand.Execute(null);
}
