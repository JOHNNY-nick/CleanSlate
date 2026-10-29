using CleanSlate.App.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>History page. Empty in J1.</summary>
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
}
