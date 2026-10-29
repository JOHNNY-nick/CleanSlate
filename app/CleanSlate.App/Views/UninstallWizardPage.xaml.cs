using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>Full uninstall wizard in 4 steps (design § 4.2).</summary>
public sealed partial class UninstallWizardPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public UninstallWizardPage()
    {
        ViewModel = App.GetService<UninstallWizardViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public UninstallWizardViewModel ViewModel { get; }
}
