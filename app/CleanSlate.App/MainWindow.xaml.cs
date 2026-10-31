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
        ["Storage"] = typeof(StoragePage),
        ["Settings"] = typeof(SettingsPage),
    };

    private const int MinimumWidth = 760;
    private const int MinimumHeight = 560;

#pragma warning disable SYSLIB1054 // LibraryImport needs unsafe code (as in CrashHandler).
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hwnd);
#pragma warning restore SYSLIB1054

    /// <summary>Creates the window and opens the home page.</summary>
    public MainWindow()
    {
        InitializeComponent();
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1200, 800));
        // Narrowest window: the open pane (320) leaves 440 px of content, where every line
        // still wraps (D57). The presenter counts physical pixels: scale by the window's DPI.
        if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter)
        {
            var scale = GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
            presenter.PreferredMinimumWidth = (int)(MinimumWidth * scale);
            presenter.PreferredMinimumHeight = (int)(MinimumHeight * scale);
        }
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "cleanslate.ico"));
    }

    /// <summary>Shows the application on its home page.</summary>
    public void ShowHome()
    {
        OnboardingFrame.Content = null;
        OnboardingFrame.Visibility = Visibility.Collapsed;
        NavView.Visibility = Visibility.Visible;
        NavView.SelectedItem = NavView.MenuItems[0];
    }

    /// <summary>
    /// First launch (D63): shows the welcome screens in place of the application and completes
    /// when the user has finished them. No page of the application is loaded before.
    /// </summary>
    public Task ShowOnboardingAsync()
    {
        var done = new TaskCompletionSource();
        NavView.Visibility = Visibility.Collapsed;
        OnboardingFrame.Visibility = Visibility.Visible;
        OnboardingFrame.Navigate(typeof(OnboardingPage));
        if (OnboardingFrame.Content is OnboardingPage page)
        {
            page.ViewModel.Completed += (_, _) => done.TrySetResult();
        }
        else
        {
            done.TrySetResult();
        }

        return done.Task;
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
            .Concat(NavView.FooterMenuItems)
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
