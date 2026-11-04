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

    /// <summary>Multicolore is shown: the paint is drawn and controls get their rounded look (D76).</summary>
    public static bool IsPainted => Paint is not null;

    /// <summary>The paint of Multicolore, or null in the other themes (D76).</summary>
    internal static PaintDecorator? Paint { get; private set; }

    private (Presentation.Services.AppTheme Theme, PaintStyle Style, bool Dark)? _applied;
    private PaintDecorator? _paint;
    private string _navSignature = string.Empty;

    // Values of App.xaml changed by Multicolore, to put them back: (dictionary, token) → color
    // of the shared brush, or the replaced object.
    private readonly Dictionary<(string Dictionary, string Token), object> _originals = [];

    /// <summary>
    /// Applies the theme of the settings (ENF-08, D71, D72, D76) to every page, at once:
    /// Multicolore recolors the Light dictionary (or the Dark one, on a dark background) and
    /// paints the window in the chosen style.
    /// </summary>
    public void ApplyTheme(Presentation.Services.AppTheme theme, PaintStyle style = PaintStyle.Splashes, bool paintDark = false)
    {
        if (Content is not Panel root || _applied == (theme, style, paintDark))
        {
            return;
        }

        _applied = (theme, style, paintDark);
        bool painted = theme == Presentation.Services.AppTheme.Colorful;
        var resources = Application.Current.Resources;
        Restore(resources);
        ResourceDictionary? palette = painted && resources.TryGetValue(ThemePalette.PaletteKey(paintDark), out object? p) ? p as ResourceDictionary : null;
        if (palette is not null)
        {
            Recolor(resources, paintDark ? "Dark" : "Light", palette);
        }

        _paint = palette is null ? null : new PaintDecorator(palette, style);
        Paint = _paint;
        root.Background = _paint is null ? null : new Microsoft.UI.Xaml.Media.SolidColorBrush(_paint.ColorOf("WindowBackgroundBrush"));
        SystemBackdrop = IsPainted ? null : SystemBackdrop ?? new Microsoft.UI.Xaml.Media.MicaBackdrop();
        var clear = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
        // Pane, content and selection without their plain boxes: the paint shows, the selected
        // item is marked by a blot (drawn in NavPaint).
        string[] boxes =
        [
            "NavigationViewDefaultPaneBackground", "NavigationViewExpandedPaneBackground", "NavigationViewContentBackground",
            "NavigationViewContentGridBorderBrush", "NavigationViewItemBackgroundSelected", "NavigationViewItemBackgroundSelectedPointerOver",
            "NavigationViewItemBackgroundSelectedPressed", "NavigationViewSelectionIndicatorForeground",
            "NavigationViewItemBackgroundPointerOver", "NavigationViewItemBackgroundPressed",
        ];
        foreach (string key in boxes)
        {
            if (IsPainted)
            {
                NavView.Resources[key] = clear;
            }
            else
            {
                NavView.Resources.Remove(key);
            }
        }

        foreach (string key in new[] { "NavigationViewItemForegroundSelected", "NavigationViewItemForegroundSelectedPointerOver", "NavigationViewItemForegroundSelectedPressed" })
        {
            if (_paint is not null)
            {
                NavView.Resources[key] = new Microsoft.UI.Xaml.Media.SolidColorBrush(_paint.ColorOf("NavActiveTextColor"));
            }
            else
            {
                NavView.Resources.Remove(key);
            }
        }

        Blobs.Children.Clear();
        NavPaint.Children.Clear();
        _navSignature = string.Empty;
        Blobs.Visibility = IsPainted ? Visibility.Visible : Visibility.Collapsed;
        NavPaint.Visibility = Blobs.Visibility;
        ContentSurface.Margin = new Thickness(0);

        ElementTheme wantedTheme = theme switch
        {
            Presentation.Services.AppTheme.Dark => ElementTheme.Dark,
            Presentation.Services.AppTheme.System => ElementTheme.Default,
            Presentation.Services.AppTheme.Colorful when paintDark => ElementTheme.Dark,
            _ => ElementTheme.Light,
        };

        // Switching away and back makes every ThemeResource look its value up again.
        root.RequestedTheme = wantedTheme == ElementTheme.Dark ? ElementTheme.Light : ElementTheme.Dark;
        root.RequestedTheme = wantedTheme;
        CurrentThemeDictionary = ThemePalette.DictionaryKey(theme, Application.Current.RequestedTheme == ApplicationTheme.Dark, paintDark);

        // The page shown is built again: badges, cards and paint follow the new theme.
        if (ContentFrame.CurrentSourcePageType is Type page)
        {
            ContentFrame.Navigate(page);
            if (ContentFrame.BackStackDepth > 0)
            {
                ContentFrame.BackStack.RemoveAt(ContentFrame.BackStackDepth - 1);
            }
        }
    }

    private void Restore(ResourceDictionary resources)
    {
        foreach (((string dictionary, string token), object original) in _originals)
        {
            if (resources.ThemeDictionaries.TryGetValue(dictionary, out object? d) && d is ResourceDictionary theme)
            {
                if (original is Windows.UI.Color color && theme[token] is Microsoft.UI.Xaml.Media.SolidColorBrush brush)
                {
                    brush.Color = color;
                }
                else
                {
                    theme[token] = original;
                }
            }
        }

        _originals.Clear();
    }

    private void Recolor(ResourceDictionary resources, string dictionary, ResourceDictionary palette)
    {
        if (!resources.ThemeDictionaries.TryGetValue(dictionary, out object? d) || d is not ResourceDictionary theme)
        {
            return;
        }

        foreach (string token in ThemePalette.Tokens)
        {
            if (!theme.TryGetValue(token, out object? current) || !palette.TryGetValue(token, out object? wanted))
            {
                continue;
            }

            if (wanted is Windows.UI.Color color && current is Microsoft.UI.Xaml.Media.SolidColorBrush brush)
            {
                // Plain color: recolor the shared brush, every element follows at once.
                _originals[(dictionary, token)] = brush.Color;
                brush.Color = color;
            }
            else
            {
                // Gradient, thickness or corner radius: swap the resource; ThemeResource
                // lookups take it again when the theme is reapplied.
                _originals[(dictionary, token)] = current;
                theme[token] = wanted;
            }
        }
    }

    private void OnContentNavigated(object sender, Microsoft.UI.Xaml.Navigation.NavigationEventArgs e)
    {
        if (_paint is null || e.Content is not Page page)
        {
            return;
        }

        // Each page its own composition, under the whole window (the menu included).
        Blobs.Children.Clear();
        UIElement background = _paint.Background(page.GetType().Name);
        if (background is FrameworkElement element)
        {
            element.Width = Blobs.ActualWidth;
            element.Height = Blobs.ActualHeight;
        }

        Blobs.Children.Add(background);
        PaintDecorator decorator = _paint;
        page.Loaded += (_, _) => decorator.Decorate(page);
    }

    private void OnBlobsSizeChanged(object sender, SizeChangedEventArgs e)
    {
        foreach (FrameworkElement child in Blobs.Children.OfType<FrameworkElement>())
        {
            child.Width = e.NewSize.Width;
            child.Height = e.NewSize.Height;
        }
    }

    // Menu (D76): a plain surface behind the items, each icon on a drop of its page color, the
    // selected item on a blot of its tint. Redrawn only when an item moved.
    private void OnNavLayoutUpdated(object? sender, object e)
    {
        if (_paint is null || NavView.Visibility != Visibility.Visible || NavPaint.XamlRoot is null)
        {
            return;
        }

        var items = NavView.MenuItems.Concat(NavView.FooterMenuItems).OfType<NavigationViewItem>()
            .Where(i => i.ActualWidth > 0 && i.XamlRoot is not null)
            .Select(i => (Item: i, Box: i.TransformToVisual(NavPaint).TransformBounds(new Windows.Foundation.Rect(0, 0, i.ActualWidth, i.ActualHeight))))
            .ToList();
        string signature = string.Join(";", items.Select(i => $"{i.Box.X:0},{i.Box.Y:0},{i.Box.Width:0}")) + (NavView.SelectedItem as NavigationViewItem)?.Tag;
        if (signature == _navSignature || items.Count == 0)
        {
            return;
        }

        _navSignature = signature;
        NavPaint.Children.Clear();
        // The menu as a paper sticker (ink outline, offset shadow): one behind the main items,
        // one behind the footer item; their text never sits on the paint.
        foreach (var group in items.GroupBy(i => NavView.FooterMenuItems.Contains(i.Item)))
        {
            double top = group.Min(i => i.Box.Top) - 10, bottom = group.Max(i => i.Box.Bottom) + 10;
            double left = group.Min(i => i.Box.Left) - 2, right = group.Max(i => i.Box.Right) + 4;
            foreach ((double shift, bool shadow) in new[] { (PaintDecorator.ShadowOffset, true), (0.0, false) })
            {
                var sticker = new Border
                {
                    Width = right - left,
                    Height = bottom - top,
                    CornerRadius = new CornerRadius(18),
                    Background = shadow ? _paint.Ink : _paint.Paper,
                    BorderBrush = shadow ? null : _paint.Ink,
                    BorderThickness = new Thickness(shadow ? 0 : 3),
                };
                Canvas.SetLeft(sticker, left + shift);
                Canvas.SetTop(sticker, top + shift);
                NavPaint.Children.Add(sticker);
            }
        }

        foreach ((NavigationViewItem item, Windows.Foundation.Rect box) in items)
        {
            if (ReferenceEquals(item, NavView.SelectedItem))
            {
                // The active item in green, as on the owner's mock-up.
                var active = new Border
                {
                    Width = box.Width - 6,
                    Height = box.Height - 4,
                    CornerRadius = new CornerRadius(12),
                    Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(_paint.ColorOf("NavActiveColor")),
                    BorderBrush = new Microsoft.UI.Xaml.Media.SolidColorBrush(_paint.ColorOf("NavActiveBorderColor")),
                    BorderThickness = new Thickness(2),
                };
                Canvas.SetLeft(active, box.X + 3);
                Canvas.SetTop(active, box.Y + 2);
                NavPaint.Children.Add(active);
            }

            if (item.Icon is FrameworkElement icon && icon.ActualWidth > 0)
            {
                Windows.Foundation.Point center = icon.TransformToVisual(NavPaint).TransformPoint(new Windows.Foundation.Point(icon.ActualWidth / 2, icon.ActualHeight / 2));
                _paint.Draw(NavPaint, [PaintComposer.Drop((item.Tag as string ?? "x")[0], center.X, center.Y, 18, 0)], _paint.ColorOf($"PageHeader{item.Tag}Brush"));
            }
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
