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

    /// <summary>
    /// Theme dictionary of App.xaml matching what the window shows (« Light » or « Dark »):
    /// converters look badges up there, not in Windows' theme (D72).
    /// </summary>
    public static string CurrentThemeDictionary { get; private set; } = "Light";

    private Presentation.Services.AppTheme _theme = Presentation.Services.AppTheme.System;

    // Light values of App.xaml changed by Multicolore, to put them back (colors and objects).
    private readonly Dictionary<string, Windows.UI.Color> _lightColors = [];
    private readonly Dictionary<string, object> _lightObjects = [];

    /// <summary>Applies the theme of the settings (ENF-08, D71, D72) to every page, at once.</summary>
    public void ApplyTheme(Presentation.Services.AppTheme theme)
    {
        if (Content is not Panel root)
        {
            return;
        }

        _theme = theme;
        bool colorful = theme == Presentation.Services.AppTheme.Colorful;
        var resources = Application.Current.Resources;
        ResourceDictionary? palette = resources.TryGetValue(Presentation.ThemePalette.ColorfulKey, out object? p) ? p as ResourceDictionary : null;
        if (resources.ThemeDictionaries.TryGetValue("Light", out object? l) && l is ResourceDictionary light && palette is not null)
        {
            foreach (string token in Presentation.ThemePalette.Tokens)
            {
                if (!light.TryGetValue(token, out object? current) || !palette.TryGetValue(token, out object? wanted))
                {
                    continue;
                }

                if (wanted is Windows.UI.Color color && current is Microsoft.UI.Xaml.Media.SolidColorBrush brush)
                {
                    // Plain color: recolor the shared brush, every element follows at once.
                    _lightColors.TryAdd(token, brush.Color);
                    brush.Color = colorful ? color : _lightColors[token];
                }
                else
                {
                    // Gradient or thickness: swap the resource; ThemeResource lookups take it
                    // again when the theme is reapplied below.
                    _lightObjects.TryAdd(token, current);
                    light[token] = colorful ? wanted : _lightObjects[token];
                }
            }
        }

        root.Background = colorful && palette is not null && palette.TryGetValue("WindowBackgroundBrush", out object? window)
            ? window as Microsoft.UI.Xaml.Media.Brush
            : null;
        SystemBackdrop = colorful ? null : SystemBackdrop ?? new Microsoft.UI.Xaml.Media.MicaBackdrop();
        ShowBlobs(colorful ? palette : null);
        ContentSurface.Margin = colorful ? new Thickness(12) : new Thickness(0);
        if (colorful && palette is not null && palette.TryGetValue("PaneBackgroundBrush", out object? pane) && pane is Windows.UI.Color paneColor)
        {
            var brush = new Microsoft.UI.Xaml.Media.SolidColorBrush(paneColor);
            NavView.Resources["NavigationViewDefaultPaneBackground"] = brush;
            NavView.Resources["NavigationViewExpandedPaneBackground"] = brush;
        }
        else
        {
            NavView.Resources.Remove("NavigationViewDefaultPaneBackground");
            NavView.Resources.Remove("NavigationViewExpandedPaneBackground");
        }

        ElementTheme wantedTheme = theme switch
        {
            Presentation.Services.AppTheme.Dark => ElementTheme.Dark,
            Presentation.Services.AppTheme.System => ElementTheme.Default,
            _ => ElementTheme.Light,
        };

        // Switching away and back makes every ThemeResource look its value up again.
        root.RequestedTheme = wantedTheme == ElementTheme.Dark ? ElementTheme.Light : ElementTheme.Dark;
        root.RequestedTheme = wantedTheme;
        CurrentThemeDictionary = Presentation.ThemePalette.DictionaryKey(theme, Application.Current.RequestedTheme == ApplicationTheme.Dark);

        // The page shown is built again: badges and card colors follow the new theme.
        if (ContentFrame.CurrentSourcePageType is Type page)
        {
            ContentFrame.Navigate(page);
            if (ContentFrame.BackStackDepth > 0)
            {
                ContentFrame.BackStack.RemoveAt(ContentFrame.BackStackDepth - 1);
            }
        }
    }

    // Big soft blobs of the five colors (Multicolore); hidden in the other themes.
    private void ShowBlobs(ResourceDictionary? palette)
    {
        Blobs.Children.Clear();
        Blobs.Visibility = palette is null ? Visibility.Collapsed : Visibility.Visible;
        if (palette is null)
        {
            return;
        }

        (double X, double Y, double Size)[] places = [(-120, -80, 520), (760, -140, 460), (180, 420, 560), (900, 360, 520), (420, 120, 380)];
        for (int i = 0; i < places.Length; i++)
        {
            if (!palette.TryGetValue($"Blob{i}", out object? value) || value is not Windows.UI.Color color)
            {
                continue;
            }

            var fill = new Microsoft.UI.Xaml.Media.RadialGradientBrush();
            fill.GradientStops.Add(new Microsoft.UI.Xaml.Media.GradientStop { Color = color, Offset = 0 });
            fill.GradientStops.Add(new Microsoft.UI.Xaml.Media.GradientStop { Color = Windows.UI.Color.FromArgb(0, color.R, color.G, color.B), Offset = 1 });
            var blob = new Microsoft.UI.Xaml.Shapes.Ellipse { Width = places[i].Size, Height = places[i].Size, Fill = fill, Opacity = 0.75 };
            Canvas.SetLeft(blob, places[i].X);
            Canvas.SetTop(blob, places[i].Y);
            Blobs.Children.Add(blob);
        }
    }

    private void OnContentNavigated(object sender, Microsoft.UI.Xaml.Navigation.NavigationEventArgs e)
    {
        if (e.Content is FrameworkElement page)
        {
            page.Loaded += (_, _) => TintCards(page);
        }
    }

    // Multicolore: every card takes its own pale color and band, in turn (D72); the other
    // themes keep the card style untouched.
    private void TintCards(FrameworkElement page)
    {
        var resources = Application.Current.Resources;
        bool colorful = _theme == Presentation.Services.AppTheme.Colorful
            && resources.TryGetValue(Presentation.ThemePalette.ColorfulKey, out object? p) && p is ResourceDictionary;
        ResourceDictionary? palette = colorful ? (ResourceDictionary)resources[Presentation.ThemePalette.ColorfulKey] : null;
        Style? cardStyle = resources.TryGetValue("CardBorderStyle", out object? s) ? s as Style : null;
        int index = 0;
        foreach (Border card in Descendants(page).OfType<Border>().Where(b => b.Style == cardStyle))
        {
            if (palette is not null
                && palette[$"CardTint{index % 5}"] is Windows.UI.Color tint
                && palette[$"CardBand{index % 5}"] is Windows.UI.Color band)
            {
                card.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(tint);
                card.BorderBrush = new Microsoft.UI.Xaml.Media.SolidColorBrush(band);
            }
            else
            {
                card.ClearValue(Border.BackgroundProperty);
                card.ClearValue(Border.BorderBrushProperty);
            }

            index++;
        }
    }

    private static IEnumerable<DependencyObject> Descendants(DependencyObject parent)
    {
        int count = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(parent);
        for (int i = 0; i < count; i++)
        {
            DependencyObject child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(parent, i);
            yield return child;
            foreach (DependencyObject below in Descendants(child))
            {
                yield return below;
            }
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

        // Already selected (e.g. back from the wizard): no selection change, open it anyway.
        if (Pages.TryGetValue(tag, out Type? page) && ContentFrame.CurrentSourcePageType != page)
        {
            ContentFrame.Navigate(page);
        }
    }

    /// <summary>Page shown in the content frame.</summary>
    public object? CurrentPage => ContentFrame.Content;

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
