using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Settings page. Empty in J1.</summary>
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
}
