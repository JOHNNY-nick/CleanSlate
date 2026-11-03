using System.Runtime.InteropServices;
using CleanSlate.App.Views;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
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

                        storage.ViewModel.Open(tile);
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

        await File.WriteAllLinesAsync(Path.Combine(folder, "capture.log"), log);
        window.Close();
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
