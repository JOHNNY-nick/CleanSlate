using System.Collections.ObjectModel;
using System.Globalization;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>One bar per drive: « 412 Go utilisés sur 512 Go — 100 Go libres ».</summary>
/// <param name="Name">« C: ».</param>
/// <param name="Text">Used and free space, in words.</param>
/// <param name="UsedPercent">Used part, 0 to 100.</param>
public sealed record DriveBar(string Name, string Text, int UsedPercent);

/// <summary>A rectangle of the map, placed by <see cref="Treemap"/>.</summary>
public sealed record StorageTile(
    double X,
    double Y,
    double Width,
    double Height,
    string Name,
    string SizeText,
    string ToolTip,
    StorageCategory Category,
    bool ShowLabel,
    DiskMapNode Node,
    bool ShowSize = true,
    bool IsLoading = false);

/// <summary>A line of the legend: category, its name is shown by the view (<c>StorageCategory_{value}</c>).</summary>
/// <param name="Category">Category.</param>
/// <param name="SizeText">Size of the category in the part shown.</param>
/// <param name="SizeBytes">Same, in bytes.</param>
public sealed record LegendItem(StorageCategory Category, string SizeText, ulong SizeBytes = 0);

/// <summary>A level of the breadcrumb.</summary>
/// <param name="Name">Name shown.</param>
/// <param name="Node">Node of the map.</param>
public sealed record BreadcrumbItem(string Name, DiskMapNode Node);

/// <summary>
/// Storage map of the « Stockage » page (D73): one bar per drive, a treemap of what takes the
/// space (by category, with a legend), as many levels to go down as the disk has (D81: a
/// folder not detailed yet gets a map of its own), with a breadcrumb. A click on leftovers or
/// an application opens its page, on a terminal folder (files only) or a file the Gestionnaire
/// with those files. Read-only: nothing is deleted from here.
/// </summary>
public sealed partial class StorageMapViewModel : ObservableObject
{
    /// <summary>Levels of one computed map; deeper, a folder gets a map of its own (D81).</summary>
    public const int MaxDepth = 3;

    // Name and size from this size on; the name alone from the smaller one.
    private const double MinLabelWidth = 90;
    private const double MinLabelHeight = 36;
    private const double MinNameWidth = 48;
    private const double MinNameHeight = 22;
    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");

    private readonly IEngineService _engine;
    private readonly IStorageService _storage;
    private readonly INavigationService _navigation;
    private readonly OperationService _operations;
    private readonly Dictionary<string, DiskMapNode> _details = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _loading = new(StringComparer.OrdinalIgnoreCase);
    private double _width;
    private double _height;

    /// <summary>Creates the ViewModel.</summary>
    public StorageMapViewModel(IEngineService engine, IStorageService storage, INavigationService navigation, OperationService operations)
    {
        _operations = operations;
        _engine = engine;
        _storage = storage;
        _navigation = navigation;
    }

    /// <summary>One bar per internal drive.</summary>
    public ObservableCollection<DriveBar> Drives { get; } = [];

    /// <summary>Rectangles of the level shown.</summary>
    public ObservableCollection<StorageTile> Tiles { get; } = [];

    /// <summary>Categories present in the level shown.</summary>
    public ObservableCollection<LegendItem> Legend { get; } = [];

    /// <summary>From the drive to the level shown.</summary>
    public ObservableCollection<BreadcrumbItem> Breadcrumb { get; } = [];

    /// <summary>Drive of the map, « C: ».</summary>
    [ObservableProperty]
    private string _drive = "C:";

    /// <summary>A map is available.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowIntro))]
    private bool _hasMap;

    /// <summary>The disk is being walked.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowIntro))]
    private bool _isComputing;

    /// <summary>« 120 000 fichiers · 80 Go comptés ».</summary>
    [ObservableProperty]
    private string _progressText = string.Empty;

    /// <summary>« Calculée le 3 nov. 2026 à 18:30 ».</summary>
    [ObservableProperty]
    private string _lastComputedText = string.Empty;

    /// <summary>« + 1,2 Go libérés par le dernier nettoyage », or empty.</summary>
    [ObservableProperty]
    private string _lastCleanupText = string.Empty;

    /// <summary>The engine failed.</summary>
    [ObservableProperty]
    private string _errorText = string.Empty;

    /// <summary>Nothing computed yet: invitation to compute the map.</summary>
    public bool ShowIntro => !HasMap && !IsComputing;

    private DiskMap? Map { get; set; }

    /// <summary>
    /// Drives, cached map and last cleanup line; the map stays as last computed, and the level
    /// shown stays where the user left it (D82: back from the Gestionnaire, at the same place).
    /// </summary>
    public void Load()
    {
        Drives.Clear();
        foreach (DriveSpace d in _storage.Drives())
        {
            Drives.Add(BarOf(d));
        }

        if (_storage.CachedMap(Drive) is { } cached)
        {
            Show(cached.Map, cached.ComputedAt, keepPlace: true);
        }

        LastCleanupText = LastCleanup(_storage.Journal());
    }

    /// <summary>« 412 Go utilisés sur 512 Go — 100 Go libres » and the used part.</summary>
    public static DriveBar BarOf(DriveSpace d)
    {
        ulong used = d.TotalBytes > d.FreeBytes ? d.TotalBytes - d.FreeBytes : 0;
        int percent = d.TotalBytes == 0 ? 0 : (int)Math.Round(100.0 * used / d.TotalBytes);
        return new DriveBar(d.Name, $"{Formats.Size(used)} utilisés sur {Formats.Size(d.TotalBytes)} — {Formats.Size(d.FreeBytes)} libres", percent);
    }

    /// <summary>Size of each category below <paramref name="node"/>, every category present.</summary>
    public static IReadOnlyList<LegendItem> CategoryTotals(DiskMapNode node)
    {
        var totals = Enum.GetValues<StorageCategory>().ToDictionary(c => c, _ => 0UL);
        AddLeaves(node, totals);
        return Enum.GetValues<StorageCategory>().Select(c => new LegendItem(c, Formats.Size(totals[c]), totals[c])).ToList();
    }

    /// <summary>Walks the drive again (read-only), with progress and cancel.</summary>
    [RelayCommand(IncludeCancelCommand = true)]
    private async Task RefreshAsync(CancellationToken cancellationToken)
    {
        IsComputing = true;
        ErrorText = string.Empty;
        ProgressText = string.Empty;
        using OperationService.Operation operation = _operations.Begin($"Carte du stockage de {Drive}", "Storage", () => RefreshCancelCommand.Execute(null));
        try
        {
            var progress = new Progress<DiskMapProgress>(p =>
            {
                ProgressText = string.Format(French, "{0:N0} fichiers · {1} comptés", p.Files, Formats.Size(p.Bytes));
                operation.Report(ProgressText);
            });
            DiskMap map = await _engine.DiskMapAsync(new DiskMapRequest(Drive + "\\"), progress, cancellationToken);
            if (!map.Cancelled)
            {
                _storage.SaveMap(Drive, map);
                Show(map, DateTimeOffset.Now);
            }
        }
        catch (EngineException ex)
        {
            ErrorText = ex.Message;
        }
        finally
        {
            IsComputing = false;
        }
    }

    /// <summary>Lays the level shown out in an area of the given size (called by the view).</summary>
    public void Arrange(double width, double height)
    {
        _width = width;
        _height = height;
        Layout();
    }

    /// <summary>
    /// A click: leftovers open « Restes orphelins », an application (a folder right below the
    /// programs folders) opens « Applications ». A folder with sub-folders goes down one level,
    /// with the results already computed; a level not computed yet gets its own map, with a
    /// loading sign in its rectangle while the engine is busy (D81, D82: no depth limit, never
    /// an error). A terminal folder (files only) or a file waits for a double-click
    /// (<see cref="DoubleClick"/>). Nothing is ever deleted from the map.
    /// </summary>
    public async Task OpenAsync(StorageTile tile, CancellationToken cancellationToken = default)
    {
        DiskMapNode node = tile.Node;
        if (node.Category == StorageCategory.Leftovers)
        {
            _navigation.OpenOrphans(false);
            return;
        }

        bool isApplication = node.Category == StorageCategory.Applications
            && Breadcrumb.Count > 1 && Breadcrumb[^1].Node.Category == StorageCategory.Applications;
        if (isApplication)
        {
            _navigation.OpenApps();
            return;
        }

        if (node.IsGroup || !node.IsFolder || IsTerminal(node) || _loading.Contains(node.Path))
        {
            return;
        }

        if (node.Children.Any(c => c.IsFolder))
        {
            GoDown(node.Name, node);
            return;
        }

        // Sub-folders not on the map yet (below the levels of the map, or a map saved before
        // D81): a map of this folder alone. The user may go elsewhere meanwhile: the level is
        // entered only if the map still shows the level of the click.
        DiskMapNode clickedFrom = Breadcrumb[^1].Node;
        if (await DetailAsync(node, cancellationToken) is { HasFolders: not false } detail && ReferenceEquals(Breadcrumb[^1].Node, clickedFrom))
        {
            GoDown(node.Name, detail);
        }
    }

    /// <summary>
    /// A double-click: a terminal folder (files only) or a file opens the Gestionnaire on the
    /// files of its folder (D82). Other rectangles have their single click.
    /// </summary>
    public void DoubleClick(StorageTile tile)
    {
        if (tile.Node is { IsGroup: false } node && (!node.IsFolder || IsTerminal(node)))
        {
            ShowFiles(tile);
        }
    }

    /// <summary>« Voir les fichiers » (right-click, D82): the files of the folder, or of the folder of a file, in the Gestionnaire.</summary>
    public void ShowFiles(StorageTile tile)
    {
        DiskMapNode node = tile.Node;
        string? folder = node.IsFolder ? node.Path : Path.GetDirectoryName(node.Path);
        if (!node.IsGroup && !string.IsNullOrEmpty(folder))
        {
            _navigation.OpenManager(folder);
        }
    }

    /// <summary>Only files below: known from the map, or from a level computed on its own.</summary>
    public bool IsTerminal(DiskMapNode node) =>
        node.IsFolder && (node.HasFolders == false || (_details.TryGetValue(node.Path, out DiskMapNode? detail) && detail.HasFolders == false));

    /// <summary>What a click does on <paramref name="node"/>, for its tooltip.</summary>
    public string ClickHint(DiskMapNode node) => node switch
    {
        { Category: StorageCategory.Leftovers } => "Clic : ouvrir Restes orphelins",
        { IsGroup: true } => string.Empty,
        { IsFolder: false } => "Double-clic : voir les fichiers de ce dossier dans le Gestionnaire",
        _ when IsTerminal(node) => "Double-clic : voir ses fichiers dans le Gestionnaire",
        _ => "Clic : détailler · clic droit : voir les fichiers",
    };

    private void GoDown(string name, DiskMapNode node)
    {
        Breadcrumb.Add(new BreadcrumbItem(name, node));
        Layout();
    }

    // A map rooted at the folder (read-only), kept for the session; null when cancelled or
    // failed. While it is computed (or waits for the engine), its rectangle shows a loading sign.
    private async Task<DiskMapNode?> DetailAsync(DiskMapNode node, CancellationToken cancellationToken)
    {
        if (_details.TryGetValue(node.Path, out DiskMapNode? known))
        {
            return known;
        }

        ErrorText = string.Empty;
        _loading.Add(node.Path);
        Layout();
        using OperationService.Operation operation = _operations.Begin($"Carte de {node.Name}", "Storage");
        try
        {
            var progress = new Progress<DiskMapProgress>(p =>
                operation.Report(string.Format(French, "{0:N0} fichiers · {1} comptés", p.Files, Formats.Size(p.Bytes))));
            DiskMap map = await _engine.DiskMapAsync(new DiskMapRequest(node.Path), progress, cancellationToken);
            if (map.Cancelled)
            {
                return null;
            }

            _details[node.Path] = map.Root;
            return map.Root;
        }
        catch (EngineException ex)
        {
            ErrorText = ex.Message;
            return null;
        }
        finally
        {
            _loading.Remove(node.Path);
            Layout();
        }
    }

    /// <summary>Goes back to a level of the breadcrumb.</summary>
    public void GoTo(BreadcrumbItem item)
    {
        int index = Breadcrumb.IndexOf(item);
        while (index >= 0 && Breadcrumb.Count > index + 1)
        {
            Breadcrumb.RemoveAt(Breadcrumb.Count - 1);
        }

        Layout();
    }

    // keepPlace: the same folders are entered again in the map read anew (sizes updated by a
    // cleanup, D73), as far as they still exist; a new walk starts from the drive.
    private void Show(DiskMap map, DateTimeOffset computedAt, bool keepPlace = false)
    {
        List<string> place = keepPlace ? Breadcrumb.Skip(1).Select(b => b.Node.Path).ToList() : [];
        Map = map;
        HasMap = true;
        LastComputedText = $"Calculée le {Formats.Date(DateOnly.FromDateTime(computedAt.LocalDateTime))} à {computedAt.LocalDateTime:HH:mm}";
        if (!keepPlace)
        {
            _details.Clear();
        }

        Breadcrumb.Clear();
        Breadcrumb.Add(new BreadcrumbItem(map.Root.Name.TrimEnd('\\'), map.Root));
        foreach (string path in place)
        {
            DiskMapNode? next = Breadcrumb[^1].Node.Children.FirstOrDefault(c => c.IsFolder && string.Equals(c.Path, path, StringComparison.OrdinalIgnoreCase));
            if (next is null)
            {
                break;
            }

            if (!next.Children.Any(c => c.IsFolder) && _details.TryGetValue(path, out DiskMapNode? detail))
            {
                next = detail with { Name = next.Name };
            }

            Breadcrumb.Add(new BreadcrumbItem(next.Name, next));
        }

        Layout();
    }

    private void Layout()
    {
        Tiles.Clear();
        Legend.Clear();
        if (Map is null || Breadcrumb.Count == 0)
        {
            return;
        }

        DiskMapNode level = Breadcrumb[^1].Node;
        ulong disk = Map.TotalBytes > 0 ? Map.TotalBytes : Map.Root.SizeBytes;
        IReadOnlyList<DiskMapNode> children = level.Children.Count > 0 ? level.Children : [level];
        if (_width > 0 && _height > 0)
        {
            foreach (TreemapRect r in Treemap.Layout(children.Select(c => c.SizeBytes).ToList(), _width, _height))
            {
                DiskMapNode c = children[r.Index];
                double share = disk == 0 ? 0 : 100.0 * c.SizeBytes / disk;
                string where = c.Path.Length > 0 ? c.Path : c.Name;
                string hint = ClickHint(c);
                Tiles.Add(new StorageTile(
                    r.X, r.Y, r.Width, r.Height, c.Name, Formats.Size(c.SizeBytes),
                    string.Format(French, "{0}\n{1} · {2:0} % du disque{3}", where, Formats.Size(c.SizeBytes), share, hint.Length > 0 ? "\n" + hint : string.Empty),
                    c.Category,
                    r.Width >= MinNameWidth && r.Height >= MinNameHeight,
                    c,
                    r.Width >= MinLabelWidth && r.Height >= MinLabelHeight,
                    _loading.Contains(c.Path)));
            }
        }

        // Every category is always named, with its size in the whole part shown (sub-folders
        // included): leftovers deep in AppData still appear in the legend of the drive.
        foreach (LegendItem item in CategoryTotals(level))
        {
            Legend.Add(item);
        }
    }

    // Children of a node add up to its size: the leaves give the size of each category.
    private static void AddLeaves(DiskMapNode node, Dictionary<StorageCategory, ulong> totals)
    {
        if (node.Children.Count == 0)
        {
            totals[node.Category] += node.SizeBytes;
            return;
        }

        foreach (DiskMapNode child in node.Children)
        {
            AddLeaves(child, totals);
        }
    }

    /// <summary>
    /// The last cleanup, in one line: space freed by a purge or an immediate deletion, or the
    /// size put in quarantine (given back at the purge); empty when nothing was cleaned.
    /// </summary>
    public static string LastCleanup(IReadOnlyList<StorageEntry> journal)
    {
        StorageEntry? last = journal
            .Where(e => e.Operation is StorageOperation.Purge or StorageOperation.DeleteForGood or StorageOperation.Quarantine)
            .OrderBy(e => e.When)
            .LastOrDefault();
        if (last is null)
        {
            return string.Empty;
        }

        return last.Operation == StorageOperation.Quarantine
            ? $"{Formats.Size(last.ReportedBytes)} mis en quarantaine par le dernier nettoyage (libérés à la purge)"
            : $"+ {Formats.Size(last.ReportedBytes)} libérés par le dernier nettoyage";
    }
}
