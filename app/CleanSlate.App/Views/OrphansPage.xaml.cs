using System.ComponentModel;
using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Orphan leftovers (EF-05 to EF-11).</summary>
public sealed partial class OrphansPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel (shared: results survive navigation).</summary>
    public OrphansPage()
    {
        ViewModel = App.GetService<OrphansViewModel>();
        InitializeComponent();
        ViewModel.PropertyChanged += OnViewModelChanged;
        Unloaded += (_, _) => ViewModel.PropertyChanged -= OnViewModelChanged;
    }

    /// <summary>Bound ViewModel.</summary>
    public OrphansViewModel ViewModel { get; }

    // Narrator reads the live region only when it is told that it changed.
    private void OnViewModelChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(OrphansViewModel.ProgressAnnouncement))
        {
            FrameworkElementAutomationPeer.FromElement(ProgressAnnouncement)
                ?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
        }
    }
}
