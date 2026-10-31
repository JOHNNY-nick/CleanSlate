using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>First-launch screens (D63): welcome, how it works and acceptance, basic settings.</summary>
public sealed partial class OnboardingPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public OnboardingPage()
    {
        ViewModel = App.GetService<OnboardingViewModel>();
        InitializeComponent();
    }

    /// <summary>Bound ViewModel.</summary>
    public OnboardingViewModel ViewModel { get; }
}
