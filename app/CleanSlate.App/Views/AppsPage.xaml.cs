using CleanSlate.App.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Apps page. Empty in J1.</summary>
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
}
