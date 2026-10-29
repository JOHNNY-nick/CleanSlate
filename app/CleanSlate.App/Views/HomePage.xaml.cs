using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Home page (dashboard).</summary>
public sealed partial class HomePage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public HomePage()
    {
        ViewModel = App.GetService<HomeViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public HomeViewModel ViewModel { get; }

    private void OnLoaded(object sender, RoutedEventArgs e) => ViewModel.LoadCommand.Execute(null);
}
