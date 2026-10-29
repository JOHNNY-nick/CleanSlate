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

    /// <summary>Applies the theme of the settings (ENF-08).</summary>
    public void ApplyTheme(Presentation.Services.AppTheme theme)
    {
        if (Content is FrameworkElement root)
        {
            root.RequestedTheme = theme switch
            {
                Presentation.Services.AppTheme.Light => ElementTheme.Light,
                Presentation.Services.AppTheme.Dark => ElementTheme.Dark,
                _ => ElementTheme.Default,
            };
        }
    }

    /// <summary>Shows the uninstall wizard (reached from the Applications page).</summary>
    public void ShowUninstallWizard() => ContentFrame.Navigate(typeof(UninstallWizardPage));

    /// <summary>Selects a page of the navigation pane by its tag (« History », « Orphans »…).</summary>
    public void ShowPage(string tag)
    {
        NavigationViewItem? item = NavView.MenuItems
            .OfType<NavigationViewItem>()
            .FirstOrDefault(i => i.Tag as string == tag);
        if (item is not null)
        {
            NavView.SelectedItem = item;
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
