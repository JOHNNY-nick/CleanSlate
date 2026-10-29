using CleanSlate.App.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Orphans page. Empty in J1.</summary>
public sealed partial class OrphansPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public OrphansPage()
    {
        ViewModel = App.GetService<OrphansViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public OrphansViewModel ViewModel { get; }
}
