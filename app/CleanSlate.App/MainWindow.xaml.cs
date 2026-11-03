using CleanSlate.App.Views;
using CleanSlate.Presentation;
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
        ContentSurface.Margin = colorful ? new Thickness(28) : new Thickness(0);
        if (colorful && palette is not null && palette.TryGetValue("PaneBackgroundBrush", out object? pane) && pane is Windows.UI.Color paneColor)
        {
            var brush = new Microsoft.UI.Xaml.Media.SolidColorBrush(paneColor);
            NavView.Resources["NavigationViewDefaultPaneBackground"] = brush;
            NavView.Resources["NavigationViewExpandedPaneBackground"] = brush;
            // The content area of the NavigationView is a veil of white: the splashes of the
            // frame show at full color without it.
            NavView.Resources["NavigationViewContentBackground"] = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
            NavView.Resources["NavigationViewContentGridBorderBrush"] = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
        }
        else
        {
            NavView.Resources.Remove("NavigationViewDefaultPaneBackground");
            NavView.Resources.Remove("NavigationViewExpandedPaneBackground");
            NavView.Resources.Remove("NavigationViewContentBackground");
            NavView.Resources.Remove("NavigationViewContentGridBorderBrush");
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

    // Paint splashes of the five colors (Multicolore, D75) around the content and in the empty
    // part of the menu; hidden in the other themes. Never under text: the content surface and
    // the menu items stay on plain white.
    private void ShowBlobs(ResourceDictionary? palette)
    {
        _splashColors = palette is null ? [] : SplashColors(palette);
        Blobs.Visibility = palette is null ? Visibility.Collapsed : Visibility.Visible;
        Blobs.SizeChanged -= OnBlobsSizeChanged;
        // The pane footer keeps its place and size in every theme, empty outside Multicolore:
        // hiding it or taking it out of the NavigationView pushed the menu items down.
        if (palette is null)
        {
            Blobs.Children.Clear();
            PaneSplash.Children.Clear();
            return;
        }

        Blobs.SizeChanged += OnBlobsSizeChanged;
        DrawFrameSplashes();
        DrawPaneSplashes();
    }

    private void OnPaneSplashSizeChanged(object sender, SizeChangedEventArgs e) => DrawPaneSplashes();

    // A large splash in the empty part of the menu, above « Réglages ».
    private void DrawPaneSplashes()
    {
        DrawSplashes(PaneSplash, new Windows.Foundation.Size(PaneSplash.ActualWidth, PaneSplash.ActualHeight),
            [(0.30, 0.45, 0.30, 0), (0.70, 0.35, 0.22, 3), (0.55, 0.80, 0.18, 1), (0.85, 0.75, 0.12, 2), (0.15, 0.85, 0.12, 4)], seed: 40);
    }

    private Windows.UI.Color[] _splashColors = [];

    private static Windows.UI.Color[] SplashColors(ResourceDictionary palette) => Enumerable.Range(0, 5)
        .Select(i => palette.TryGetValue($"Blob{i}", out object? value) && value is Windows.UI.Color color ? color : default)
        .ToArray();

    private void OnBlobsSizeChanged(object sender, SizeChangedEventArgs e) => DrawFrameSplashes();

    // Splashes along the edges of the window: they show in the frame around the content.
    private void DrawFrameSplashes() => DrawSplashes(Blobs, new Windows.Foundation.Size(Blobs.ActualWidth, Blobs.ActualHeight),
        [(0.40, 0.00, 0.10, 0), (0.70, 0.01, 0.12, 1), (0.99, 0.10, 0.12, 2), (0.995, 0.55, 0.10, 3), (0.97, 0.99, 0.12, 4), (0.62, 1.00, 0.10, 0), (0.36, 0.99, 0.09, 2)], seed: 10);

    // Each splash: (center x, center y, radius) in parts of the area (radius of its smaller
    // side), and its color among Blob0 to Blob4.
    private void DrawSplashes(Canvas canvas, Windows.Foundation.Size size, (double X, double Y, double R, int Color)[] splashes, int seed)
    {
        canvas.Children.Clear();
        canvas.Clip = new Microsoft.UI.Xaml.Media.RectangleGeometry { Rect = new Windows.Foundation.Rect(0, 0, size.Width, size.Height) };
        double side = Math.Min(size.Width, size.Height) > 0 ? Math.Max(Math.Min(size.Width, size.Height), 400) : 0;
        for (int i = 0; i < splashes.Length && side > 0 && _splashColors.Length == 5; i++)
        {
            var fill = new Microsoft.UI.Xaml.Media.SolidColorBrush(_splashColors[splashes[i].Color]);
            foreach (SplatterDot dot in Splatter.Generate(seed + i, splashes[i].X * size.Width, splashes[i].Y * size.Height, splashes[i].R * side))
            {
                var ellipse = new Microsoft.UI.Xaml.Shapes.Ellipse
                {
                    Width = dot.Width,
                    Height = dot.Height,
                    Fill = fill,
                    Opacity = 0.9,
                    RenderTransformOrigin = new Windows.Foundation.Point(0.5, 0.5),
                    RenderTransform = new Microsoft.UI.Xaml.Media.RotateTransform { Angle = dot.Angle },
                };
                Canvas.SetLeft(ellipse, dot.X);
                Canvas.SetTop(ellipse, dot.Y);
                canvas.Children.Add(ellipse);
            }
        }
    }

    // Multicolore: a splash on the right of each page header; the title keeps the plain
    // color of the header behind it, so that no letter sits on the paint.
    private void SplashHeaders(FrameworkElement page)
    {
        Style? headerStyle = Application.Current.Resources.TryGetValue("PageHeaderBorderStyle", out object? h) ? h as Style : null;
        foreach (Border header in Descendants(page).OfType<Border>().Where(b => b.Style == headerStyle).ToList())
        {
            if (header.Child is not TextBlock title)
            {
                continue;
            }

            header.Child = null;
            var splash = new Canvas { IsHitTestVisible = false };
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetAccessibilityView(splash, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
            splash.SizeChanged += (_, e) => DrawSplashes(splash, e.NewSize,
                [(0.93, 0.5, 0.11, 1), (0.80, 0.3, 0.07, 3), (0.70, 0.75, 0.05, 2), (0.86, 0.95, 0.04, 4), (0.62, 0.2, 0.035, 0)], seed: 70);
            var grid = new Grid();
            grid.Children.Add(splash);
            grid.Children.Add(new Border
            {
                Background = header.Background,
                CornerRadius = new CornerRadius(6),
                HorizontalAlignment = HorizontalAlignment.Left,
                Padding = new Thickness(0, 0, 16, 0),
                Child = title,
            });
            header.Child = grid;
        }
    }

    private void OnContentNavigated(object sender, Microsoft.UI.Xaml.Navigation.NavigationEventArgs e)
    {
        if (e.Content is FrameworkElement page)
        {
            page.Loaded += (_, _) =>
            {
                TintCards(page);
                if (_theme == Presentation.Services.AppTheme.Colorful)
                {
                    SplashHeaders(page);
                }
            };
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
