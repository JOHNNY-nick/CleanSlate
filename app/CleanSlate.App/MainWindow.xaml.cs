using CleanSlate.App.Views;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App;

/// <summary>Main window: navigation pane on the left, pages on the right.</summary>
public sealed partial class MainWindow : Window
{
    private static readonly Dictionary<string, Type> Pages = new()
    {
        ["Home"] = typeof(HomePage),
        ["Apps"] = typeof(AppsPage),
        ["Orphans"] = typeof(OrphansPage),
        ["History"] = typeof(HistoryPage),
        ["Settings"] = typeof(SettingsPage),
    };

    /// <summary>Creates the window and opens the home page.</summary>
    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1200, 800));
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "cleanslate.ico"));
        NavView.SelectedItem = NavView.MenuItems[0];
    }

    /// <summary>Shows the uninstall wizard (reached from the Applications page).</summary>
    public void ShowUninstallWizard() => ContentFrame.Navigate(typeof(UninstallWizardPage));

    /// <summary>Selects « Historique » in the navigation pane.</summary>
    public void ShowHistory()
    {
        NavigationViewItem? history = NavView.MenuItems
            .OfType<NavigationViewItem>()
            .FirstOrDefault(i => i.Tag as string == "History");
        if (history is not null)
        {
            NavView.SelectedItem = history;
        }
    }

    private void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem { Tag: string tag }
            && Pages.TryGetValue(tag, out Type? page)
            && ContentFrame.CurrentSourcePageType != page)
        {
            ContentFrame.Navigate(page);
        }
    }
}
