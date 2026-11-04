using System.Runtime.InteropServices;
using CleanSlate.App.Views;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace CleanSlate.App;

/// <summary>
/// Screenshots of the development mode (D74): every page in Clair, Sombre and Multicolore, plus
/// the wizard and a detail level of the storage map, saved as PNG, then the application closes.
/// Runs only in simulation (<see cref="Presentation.DevOptions"/>): the engine simulates every
/// action, and the capture itself only reads (orphans scan, storage map, list of applications).
/// </summary>
internal static class Capture
{
    private static readonly (AppTheme Theme, string Name)[] Themes =
    [
        (AppTheme.Light, "clair"),
        (AppTheme.Dark, "sombre"),
        (AppTheme.Colorful, "multicolore"),
    ];

    private static readonly string[] Tags = ["Home", "Apps", "Orphans", "History", "Storage", "Settings"];

    /// <summary>Takes every screenshot into <paramref name="folder"/>, writes <c>capture.log</c>, closes the window.</summary>
    public static async Task RunAsync(MainWindow window, string folder)
    {
        Directory.CreateDirectory(folder);
        var log = new List<string>();
        var settings = App.GetService<ISettingsService>();
        var engine = App.GetService<IEngineService>();
        window.AppWindow.Resize(new Windows.Graphics.SizeInt32(1400, 1050)); // the map and its legend fit
        try
        {
            // Read-only preparation: a scan (for the confidence badges) and the map of C:.
            var started = DateTimeOffset.Now;
            await App.GetService<OrphansViewModel>().ScanCommand.ExecuteAsync(null);
            log.Add($"scan: {(DateTimeOffset.Now - started).TotalSeconds:0} s");
            started = DateTimeOffset.Now;
            DiskMap map = await engine.DiskMapAsync(new DiskMapRequest(@"C:\"), null, CancellationToken.None);
            App.GetService<IStorageService>().SaveMap("C:", map);
            log.Add($"carte: {(DateTimeOffset.Now - started).TotalSeconds:0} s, {map.Denied} dossiers refusés");
            InstalledApp? app = (await engine.ListAppsAsync(CancellationToken.None)).FirstOrDefault(a => a.HiddenReason is null);
            if (DevMode.Options.CaptureSet == "corrections")
            {
                await Corrections(window, Path.Combine(folder, "corrections"), settings, log);
                return;
            }

            if (DevMode.Options.CaptureSet == "gestionnaire")
            {
                await ManagerTabs(window, Path.Combine(folder, "gestionnaire"), settings, log);
                return;
            }

            if (DevMode.Options.CaptureSet == "multicolore-v2")
            {
                await AllPages(window, Path.Combine(folder, "multicolore-v2"), settings, log);
                return;
            }

            if (DevMode.Options.CaptureSet == "multicolore")
            {
                await MockUp(window, Path.Combine(folder, "multicolore"), settings, log);
                return;
            }

            foreach ((AppTheme theme, string name) in Themes)
            {
                settings.Theme = theme; // applied by MainWindow, also shown checked in Réglages
                await Task.Delay(1000);
                foreach (string tag in Tags)
                {
                    window.ShowPage(tag);
                    try
                    {
                        await Save(window, Path.Combine(folder, $"{name}-{tag}.png"), log);
                    }
                    catch (Exception ex)
                    {
                        log.Add($"{name}-{tag}: {ex}");
                    }
                }

                window.ShowPage("Storage");
                await Task.Delay(1500);
                if (window.CurrentPage is StoragePage storage)
                {
                    // At least one level down the largest folder, further while a level has few rectangles.
                    for (int level = 0; level < StorageMapViewModel.MaxDepth && (level == 0 || storage.ViewModel.Tiles.Count < 4); level++)
                    {
                        if (storage.ViewModel.Tiles.Where(t => t.Node.IsFolder && t.Node.Children.Count > 0).MaxBy(t => t.Node.SizeBytes) is not { } tile)
                        {
                            break;
                        }

                        await storage.ViewModel.OpenAsync(tile);
                    }

                    await Save(window, Path.Combine(folder, $"{name}-Storage-detail.png"), log);
                }

                if (app is not null)
                {
                    App.GetService<INavigationService>().OpenUninstallWizard(app, complete: false);
                    await Save(window, Path.Combine(folder, $"{name}-Wizard.png"), log);
                }
            }
        }
        catch (Exception ex)
        {
            log.Add(ex.ToString());
        }
        finally
        {
            await File.WriteAllLinesAsync(Path.Combine(folder, "capture.log"), log);
            window.Close();
        }
    }

    // Every tab of the Gestionnaire (D79), in Clair, after a read-only analysis of C:.
    private static async Task ManagerTabs(MainWindow window, string folder, ISettingsService settings, List<string> log)
    {
        Directory.CreateDirectory(folder);
        settings.Theme = AppTheme.Light;
        await Task.Delay(1000);
        window.ShowPage("Manager");
        await Task.Delay(3000);
        if (window.CurrentPage is not ManagerPage page)
        {
            log.Add("Gestionnaire introuvable");
            return;
        }

        var started = DateTimeOffset.Now;
        page.ViewModel.AnalysisRoot = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile); // short run
        await page.ViewModel.AnalyzeCommand.ExecuteAsync(null);
        await page.ViewModel.FindDuplicatesCommand.ExecuteAsync(null);
        log.Add($"analyse du Gestionnaire : {(DateTimeOffset.Now - started).TotalSeconds:0} s");
        await Tabs(window, page.ViewModel, folder, string.Empty, log);
    }

    // Every tab of the Gestionnaire (D82: the « Carte » tab is gone, the map is the Stockage page).
    private static async Task Tabs(MainWindow window, ManagerViewModel vm, string folder, string prefix, List<string> log)
    {
        string[] names = ["diagnostic", "recherche", "categories", "gros-fichiers", "anciens", "doublons", "systeme", "en-attente", "evolution", "volumes"];
        for (int i = 0; i < names.Length; i++)
        {
            vm.TabIndex = i;
            if (i == 1)
            {
                vm.Search = "Downloads";
            }

            if (i == 2)
            {
                vm.CategoryIndex = 0;
            }

            await Save(window, Path.Combine(folder, $"{prefix}{i:00}-{names[i]}.png"), log);
        }
    }

    private static async Task Show(MainWindow window, string tag)
    {
        for (int attempt = 0; attempt < 5 && window.CurrentPage?.GetType().Name != $"{tag}Page"; attempt++)
        {
            window.ShowPage(tag);
            await Task.Delay(700);
        }
    }

    // The corrections after the first use (D82, D83), in Multicolore: the menu in its three
    // markings and its button (7, 8), an analysis seen from another page and on the way back
    // (3), every tab of the Gestionnaire and the help of its actions (2, 6), and the map with
    // the explorer under it, down to a terminal folder (4, 5, D83).
    private static async Task Corrections(MainWindow window, string folder, ISettingsService settings, List<string> log)
    {
        Directory.CreateDirectory(folder);
        settings.PaintStyle = Presentation.PaintStyle.Splashes;
        settings.PaintDark = false;
        settings.Theme = AppTheme.Colorful;
        await Task.Delay(1000);
        foreach ((MainWindow.NavMarker marker, string name) in new[] { (MainWindow.NavMarker.Sticker, "autocollant"), (MainWindow.NavMarker.Brush, "pinceau"), (MainWindow.NavMarker.Splash, "eclaboussure") })
        {
            window.Marker = marker;
            foreach (string tag in new[] { "Home", "Storage" })
            {
                await Show(window, tag);
                await Save(window, Path.Combine(folder, $"menu-{name}-{tag}.png"), log);
            }
        }

        window.Marker = MainWindow.NavMarker.Sticker;

        // 8: the menu button in Clair and Sombre too (D83: no offset shadow out of Multicolore).
        foreach ((AppTheme theme, string name) in new[] { (AppTheme.Light, "clair"), (AppTheme.Dark, "sombre") })
        {
            settings.Theme = theme;
            await Task.Delay(1000);
            await Show(window, "Home");
            await Save(window, Path.Combine(folder, $"bouton-menu-{name}.png"), log);
        }

        settings.Theme = AppTheme.Colorful;
        await Task.Delay(1000);

        // 3: an analysis keeps running when the page is left, and is found again.
        await Show(window, "Manager");
        ManagerViewModel manager = App.GetService<ManagerViewModel>();
        manager.AnalysisRoot = Path.GetPathRoot(Environment.SystemDirectory) ?? @"C:\";
        var started = DateTimeOffset.Now;
        Task analysis = manager.AnalyzeCommand.ExecuteAsync(null);
        await Task.Delay(3000);
        await Show(window, "Home");
        await Save(window, Path.Combine(folder, "analyse-en-cours-depuis-accueil.png"), log);
        await Show(window, "Manager");
        await Save(window, Path.Combine(folder, "analyse-en-cours-au-retour.png"), log);
        await analysis;
        await manager.FindDuplicatesCommand.ExecuteAsync(null);
        log.Add($"analyse du Gestionnaire : {(DateTimeOffset.Now - started).TotalSeconds:0} s");

        // 2, 6: the tabs, then the help of the action bar.
        await Show(window, "Manager");
        await Tabs(window, manager, folder, "gestionnaire-", log);
        manager.TabIndex = 0;
        if (window.CurrentPage is ManagerPage managerPage
            && PaintDecorator.Descendants(managerPage).OfType<Button>().FirstOrDefault(b => Microsoft.UI.Xaml.Automation.AutomationProperties.GetName(b) == "Aide sur les actions") is { Flyout: { } help } helpButton)
        {
            help.ShowAt(helpButton);
            await Save(window, Path.Combine(folder, "gestionnaire-aide-des-actions.png"), log);
            help.Hide();
        }

        // D83: the map and the explorer under it, level after level: Program Files (x86) and
        // an application folder (browsed now), then down the largest folders to a terminal one.
        await Show(window, "Storage");
        await Task.Delay(1500);
        if (window.CurrentPage is not StoragePage storage)
        {
            log.Add("Stockage introuvable");
            return;
        }

        StorageMapViewModel map = storage.ViewModel;
        for (int wait = 0; wait < 40 && map.Tiles.Count == 0; wait++)
        {
            await Task.Delay(250); // the map is laid out once its area has a size
        }

        ScrollViewer? scroll = PaintDecorator.Descendants(storage).OfType<ScrollViewer>().FirstOrDefault();
        async Task Both(string name)
        {
            await map.ListingTask;
            scroll?.ChangeView(null, 0, null, true);
            await Save(window, Path.Combine(folder, $"carte-{name}.png"), log);
            scroll?.ChangeView(null, scroll.ScrollableHeight, null, true);
            await Save(window, Path.Combine(folder, $"explorateur-{name}.png"), log);
        }

        if (map.Tiles.Where(t => t.Name.StartsWith("Program Files", StringComparison.OrdinalIgnoreCase)).MaxBy(t => t.Node.SizeBytes) is { } programs)
        {
            await map.OpenAsync(programs);
            await Both("program-files");
            if (map.Tiles.Where(t => t.Node.IsFolder && !t.Node.IsGroup).MaxBy(t => t.Node.SizeBytes) is { } application)
            {
                log.Add($"application : {application.Node.Path}");
                await map.OpenAsync(application);
                await Both("application");
            }
        }

        map.GoTo(map.Breadcrumb[0]);
        for (int level = 0; level < 8; level++)
        {
            if (map.Tiles.Where(t => !t.Node.IsGroup && map.IsTerminal(t.Node)).MaxBy(t => t.Node.SizeBytes) is { } terminal)
            {
                log.Add($"dossier terminal : {terminal.Node.Path}");
                await map.OpenAsync(terminal);
                await Both("dossier-terminal");
                break;
            }

            if (map.Tiles.Where(t => t.Node.IsFolder && !t.Node.IsGroup).MaxBy(t => t.Node.SizeBytes) is not { } next)
            {
                break;
            }

            await map.OpenAsync(next);
            await Both($"niveau-{level + 1}");
        }

        // Back on the page: the same place (the ViewModel outlives the page).
        await Show(window, "Home");
        await Show(window, "Storage");
        await Task.Delay(1500);
        await Save(window, Path.Combine(folder, "carte-retour-meme-endroit.png"), log);
    }

    // Every page in Multicolore (D76), on white and on dark, with the check of every text.
    private static async Task AllPages(MainWindow window, string folder, ISettingsService settings, List<string> log)
    {
        Directory.CreateDirectory(folder);
        Report = [];
        foreach (bool dark in new[] { false, true })
        {
            settings.PaintStyle = Presentation.PaintStyle.Splashes;
            settings.PaintDark = dark;
            settings.Theme = AppTheme.Colorful;
            await Task.Delay(1000);
            foreach (string tag in Tags)
            {
                for (int attempt = 0; attempt < 5 && window.CurrentPage?.GetType().Name != $"{tag}Page"; attempt++)
                {
                    window.ShowPage(tag);
                    await Task.Delay(700);
                }

                log.Add($"{tag} : page {window.CurrentPage?.GetType().Name} avant la capture");
                await Save(window, Path.Combine(folder, $"{(dark ? "sombre" : "clair")}-{tag}.png"), log);
                log.Add($"{tag} : page {window.CurrentPage?.GetType().Name} après la capture");
            }
        }

        int failures = Report.Count(l => l.StartsWith("ÉCHEC", StringComparison.Ordinal));
        Report.Insert(0, $"{failures} texte(s) en échec, {Report.Count} mesuré(s) : contraste ≥ {Presentation.TextMeasure.MinRatio}:1 sur un fond uni à ≥ {Presentation.TextMeasure.MinPlainness:P0}.");
        await File.WriteAllLinesAsync(Path.Combine(folder, "contraste.txt"), Report);
        log.Add($"contraste : {failures} échec(s)");
        Report = null;
    }

    // Lines of the text check, while a set that checks texts runs.
    private static List<string>? Report { get; set; }

    // Every visible text of the window: the background under it must be plain and contrasted.
    private static void CheckTexts(MainWindow window, byte[] pixels, int width, string shot)
    {
        if (window.Content is not UIElement root || Report is null)
        {
            return;
        }

        int height = pixels.Length / 4 / width;
        double scale = window.Content.XamlRoot?.RasterizationScale ?? 1;
        foreach (TextBlock text in PaintDecorator.Descendants(root).OfType<TextBlock>())
        {
            if (text.ActualWidth < 4 || text.ActualHeight < 4 || string.IsNullOrWhiteSpace(text.Text) || text.Foreground is not SolidColorBrush brush || !IsShown(text))
            {
                continue;
            }

            Windows.Foundation.Rect box = text.TransformToVisual(root).TransformBounds(new Windows.Foundation.Rect(0, 0, text.ActualWidth, text.ActualHeight));
            int left = (int)(box.X * scale), top = (int)(box.Y * scale), w = (int)(box.Width * scale), h = (int)(box.Height * scale);
            if (left < 0 || top < 0 || left + w > width || top + h > height || Clipped(text, root, box))
            {
                continue; // scrolled out of the window or of its list
            }

            Presentation.TextMeasure m = Presentation.TextContrast.Measure(pixels, width, left, top, w, h, (brush.Color.R, brush.Color.G, brush.Color.B));
            // A glyph of Segoe Fluent Icons is an icon, not text: 3:1 is asked of icons (WCAG 1.4.11).
            bool icon = text.Text.All(c => c is >= (char)0xE000 and <= (char)0xF8FF);
            bool passes = icon ? m.Ratio >= 3 : m.Passes;
            string name = icon ? "icône" : text.Text.Length > 40 ? text.Text[..40] + "…" : text.Text;
            Report.Add($"{(passes ? "ok" : "ÉCHEC")} | {shot} | {m.Ratio:0.0}:1 | uni {m.Plainness:P0} | fond {m.Background} | « {name} »");
        }
    }

    // Partly hidden by a scrolled area around it (a list row cut at the bottom).
    private static bool Clipped(FrameworkElement element, UIElement root, Windows.Foundation.Rect box)
    {
        for (DependencyObject? e = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(element); e is not null; e = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(e))
        {
            if (e is ScrollViewer viewer && viewer.ActualWidth > 0)
            {
                Windows.Foundation.Rect view = viewer.TransformToVisual(root).TransformBounds(new Windows.Foundation.Rect(0, 0, viewer.ActualWidth, viewer.ActualHeight));
                if (box.Top < view.Top - 1 || box.Bottom > view.Bottom + 1 || box.Left < view.Left - 1 || box.Right > view.Right + 1)
                {
                    return true;
                }
            }
        }

        return false;
    }

    // Visible, along with every parent.
    private static bool IsShown(FrameworkElement element)
    {
        for (DependencyObject? e = element; e is not null; e = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(e))
        {
            if (e is UIElement { Visibility: Visibility.Collapsed } || e is UIElement { Opacity: < 0.5 })
            {
                return false;
            }
        }

        return true;
    }

    // Mock-up of Multicolore (D76): Accueil, Applications and Stockage in each style, on white
    // and on a dark background, for the owner to choose.
    private static async Task MockUp(MainWindow window, string folder, ISettingsService settings, List<string> log)
    {
        Directory.CreateDirectory(folder);
        (Presentation.PaintStyle Style, string Name)[] styles =
            [(Presentation.PaintStyle.Splashes, "eclaboussures"), (Presentation.PaintStyle.Watercolor, "aquarelle"), (Presentation.PaintStyle.Explosion, "explosion")];
        foreach ((Presentation.PaintStyle style, string name) in styles)
        {
            foreach (bool dark in new[] { false, true })
            {
                settings.PaintStyle = style;
                settings.PaintDark = dark;
                settings.Theme = AppTheme.Colorful;
                await Task.Delay(1000);
                foreach (string tag in new[] { "Home", "Apps", "Storage" })
                {
                    // A theme applied late rebuilds the page shown before: ask until this one is there.
                    for (int attempt = 0; attempt < 5 && window.CurrentPage?.GetType().Name != $"{tag}Page"; attempt++)
                    {
                        window.ShowPage(tag);
                        await Task.Delay(700);
                    }

                    await Save(window, Path.Combine(folder, $"{name}-{(dark ? "sombre" : "clair")}-{tag}.png"), log);
                }
            }
        }
    }

    // Lets the page load, then copies the window as Windows composes it (Mica included).
    private static async Task Save(MainWindow window, string path, List<string> log)
    {
        await Task.Delay(2500);
        IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(window);
        // A minimized window has no client area to copy: bring it back, a few times at most.
        int width = 0, height = 0;
        for (int attempt = 0; attempt < 6 && (width <= 0 || height <= 0); attempt++)
        {
            if (attempt > 0)
            {
                (window.AppWindow.Presenter as Microsoft.UI.Windowing.OverlappedPresenter)?.Restore();
                window.Activate();
                await Task.Delay(1500);
            }

            width = window.AppWindow.ClientSize.Width;
            height = window.AppWindow.ClientSize.Height;
        }

        if (width <= 0 || height <= 0)
        {
            log.Add($"{Path.GetFileName(path)}: fenêtre sans surface");
            return;
        }

        byte[] pixels = new byte[width * height * 4];
        IntPtr screen = GetDC(hwnd);
        IntPtr dc = CreateCompatibleDC(screen);
        IntPtr bitmap = CreateCompatibleBitmap(screen, width, height);
        IntPtr old = SelectObject(dc, bitmap);
        try
        {
            PrintWindow(hwnd, dc, PwClientOnly | PwRenderFullContent);
            var header = new BitmapInfoHeader { Size = 40, Width = width, Height = -height, Planes = 1, BitCount = 32 };
            GetDIBits(dc, bitmap, 0, (uint)height, pixels, ref header, 0);
        }
        finally
        {
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(hwnd, screen);
        }

        for (int i = 3; i < pixels.Length; i += 4)
        {
            pixels[i] = 255; // GDI leaves the alpha byte empty
        }

        if (Report is not null)
        {
            CheckTexts(window, pixels, width, Path.GetFileNameWithoutExtension(path));
        }

        using var stream = new InMemoryRandomAccessStream();
        BitmapEncoder encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, (uint)width, (uint)height, 96, 96, pixels);
        await encoder.FlushAsync();
        stream.Seek(0);
        await using FileStream file = File.Create(path);
        await stream.AsStreamForRead().CopyToAsync(file);
        log.Add(Path.GetFileName(path));
    }

    private const uint PwClientOnly = 0x1;
    private const uint PwRenderFullContent = 0x2;

    [StructLayout(LayoutKind.Sequential)]
    private struct BitmapInfoHeader
    {
        public uint Size;
        public int Width, Height;
        public ushort Planes, BitCount;
        public uint Compression, SizeImage;
        public int XPelsPerMeter, YPelsPerMeter;
        public uint ClrUsed, ClrImportant;
    }

#pragma warning disable SYSLIB1054 // LibraryImport needs unsafe code (as in MainWindow).
    [DllImport("user32.dll")]
    private static extern IntPtr GetDC(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);

    [DllImport("user32.dll")]
    private static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);

    [DllImport("gdi32.dll")]
    private static extern IntPtr CreateCompatibleDC(IntPtr dc);

    [DllImport("gdi32.dll")]
    private static extern IntPtr CreateCompatibleBitmap(IntPtr dc, int width, int height);

    [DllImport("gdi32.dll")]
    private static extern IntPtr SelectObject(IntPtr dc, IntPtr gdiObject);

    [DllImport("gdi32.dll")]
    private static extern bool DeleteObject(IntPtr gdiObject);

    [DllImport("gdi32.dll")]
    private static extern bool DeleteDC(IntPtr dc);

    [DllImport("gdi32.dll")]
    private static extern int GetDIBits(IntPtr dc, IntPtr bitmap, uint start, uint lines, byte[] bits, ref BitmapInfoHeader header, uint usage);
#pragma warning restore SYSLIB1054
}
