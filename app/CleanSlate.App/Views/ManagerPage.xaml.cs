using CleanSlate.Interop;
using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;

namespace CleanSlate.App.Views;

/// <summary>
/// « Gestionnaire » (D79). The code-behind only switches the tab shown and forwards the
/// buttons of the templates to the ViewModel.
/// </summary>
public sealed partial class ManagerPage : Page
{
    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public ManagerPage()
    {
        ViewModel = App.GetService<ManagerViewModel>();
        Resources["SafeText"] = new Converter(v => v is true ? "sans risque" : "à vérifier");
        Resources["SystemName"] = new Converter(v => v is SystemTask t ? ManagerViewModel.SystemName(t) : string.Empty);
        Resources["SystemExplanation"] = new Converter(v => v is SystemTask t ? ManagerViewModel.SystemExplanation(t) : string.Empty);
        InitializeComponent();
        ViewModel.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(ManagerViewModel.TabIndex) && Tabs.Items.Count > ViewModel.TabIndex)
            {
                Tabs.SelectedItem = Tabs.Items[ViewModel.TabIndex];
            }
        };
    }

    /// <summary>Bound ViewModel.</summary>
    public ManagerViewModel ViewModel { get; }

    private async void OnLoaded(object sender, RoutedEventArgs e) => await ViewModel.LoadCommand.ExecuteAsync(null);

    private void OnTabChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        int index = sender.Items.IndexOf(sender.SelectedItem);
        ViewModel.TabIndex = index;
        for (int i = 0; i <= 10; i++)
        {
            if (FindName($"Tab{i}") is UIElement tab)
            {
                tab.Visibility = i == index ? Visibility.Visible : Visibility.Collapsed;
            }
        }
    }

    private async void OnFree(object sender, RoutedEventArgs e) =>
        await ViewModel.FreeCommand.ExecuteAsync((sender as FrameworkElement)?.Tag as RecoverableLine);

    private async void OnRunSystem(object sender, RoutedEventArgs e) =>
        await ViewModel.RunSystemCommand.ExecuteAsync((sender as FrameworkElement)?.Tag as SystemRow);

    // A text of the page computed by the ViewModel (no business logic here).
    private sealed partial class Converter(Func<object?, string> convert) : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language) => convert(value);

        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }
}
