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
    }

    /// <summary>Bound ViewModel.</summary>
    public ManagerViewModel ViewModel { get; }

    // The ViewModel outlives the page (D82): back here, the tab, the analysis running and its
    // progress are where they were. The page listens to it only while it is shown.
    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        Bindings.Update(); // lists bound again if the page was left and shown anew
        ViewModel.PropertyChanged -= OnViewModelChanged;
        ViewModel.PropertyChanged += OnViewModelChanged;
        ShowTab(ViewModel.TabIndex);
        await ViewModel.LoadCommand.ExecuteAsync(null);
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        ViewModel.PropertyChanged -= OnViewModelChanged;
        Bindings.StopTracking();
        BoundLists.Release(this);
    }

    private void OnViewModelChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ManagerViewModel.TabIndex))
        {
            ShowTab(ViewModel.TabIndex);
        }
    }

    private void ShowTab(int index)
    {
        if (index >= 0 && index < Tabs.Items.Count && !ReferenceEquals(Tabs.SelectedItem, Tabs.Items[index]))
        {
            Tabs.SelectedItem = Tabs.Items[index];
        }
    }

    // « ? » of the action bar: the explanation of every action, the same text as its tooltip.
    private void OnHelpOpening(object? sender, object e)
    {
        HelpPanel.Children.Clear();
        HelpPanel.Children.Add(new TextBlock { Text = Converters.Texts.Get("ManagerHelpTitle"), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextWrapping = TextWrapping.Wrap });
        foreach (Button button in ActionBar.Children.OfType<Button>())
        {
            if (ToolTipService.GetToolTip(button) is not string explanation)
            {
                continue;
            }

            // The text of the button, not the glyph of its icon (Segoe Fluent Icons, private use area).
            string name = button.Content as string
                ?? PaintDecorator.Descendants(button).OfType<TextBlock>().Select(t => t.Text).FirstOrDefault(t => t.Length > 0 && !t.All(c => c is >= (char)0xE000 and <= (char)0xF8FF))
                ?? string.Empty;
            var item = new StackPanel { Spacing = 2 };
            item.Children.Add(new TextBlock { Text = name, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextWrapping = TextWrapping.Wrap });
            item.Children.Add(new TextBlock { Text = explanation, Style = (Style)Application.Current.Resources["SecondaryTextBlockStyle"], TextWrapping = TextWrapping.Wrap });
            HelpPanel.Children.Add(item);
        }
    }

    private void OnTabChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        int index = sender.Items.IndexOf(sender.SelectedItem);
        ViewModel.TabIndex = index;
        for (int i = 0; i < sender.Items.Count; i++)
        {
            if (FindName($"Tab{i}") is UIElement tab)
            {
                tab.Visibility = i == index ? Visibility.Visible : Visibility.Collapsed;
            }
        }
    }

    private async void OnFree(object sender, RoutedEventArgs e) =>
        await ViewModel.FreeCommand.ExecuteAsync((sender as FrameworkElement)?.Tag as RecoverableLine);

    private async void OnCancelRestart(object sender, RoutedEventArgs e) =>
        await ViewModel.CancelAtRestartCommand.ExecuteAsync((sender as FrameworkElement)?.Tag as string);

    private async void OnRunSystem(object sender, RoutedEventArgs e) =>
        await ViewModel.RunSystemCommand.ExecuteAsync((sender as FrameworkElement)?.Tag as SystemRow);

    // A text of the page computed by the ViewModel (no business logic here).
    private sealed partial class Converter(Func<object?, string> convert) : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language) => convert(value);

        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }
}
