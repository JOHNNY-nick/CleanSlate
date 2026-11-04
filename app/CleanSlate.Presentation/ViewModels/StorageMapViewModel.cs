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

/// <summary>A line of the explorer under the map (D83): a folder or a file of the level shown.</summary>
public sealed partial class ExplorerRow : ObservableObject
{
    /// <summary>Wraps a child of the folder shown; <paramref name="sizeBytes"/> comes from the map for a folder it knows.</summary>
    public ExplorerRow(FolderEntry entry, ulong sizeBytes)
    {
        Entry = entry;
        SizeBytes = sizeBytes;
        SizeText = Formats.Size(sizeBytes);
        DateText = entry.Modified is { } m ? Formats.Date(DateOnly.FromDateTime(m.LocalDateTime)) : Formats.Unknown;
        TypeText = entry.IsFolder ? "Dossier" : ManagerViewModel.TypeText(entry.Path, entry.Category);
    }

    /// <summary>What the engine listed.</summary>
    public FolderEntry Entry { get; }

    /// <summary>Name.</summary>
    public string Name => Entry.Name;

    /// <summary>Full path.</summary>
    public string Path => Entry.Path;

    /// <summary>A folder: a click opens it, on the map and here.</summary>
    public bool IsFolder => Entry.IsFolder;

    /// <summary>Sensitive zone: deleting it needs the expert mode and « CONFIRMER ».</summary>
    public bool IsSensitive => Entry.Zone == Zone.Sensitive;

    /// <summary>Bytes on the disk.</summary>
    public ulong SizeBytes { get; }

    /// <summary>« 1,2 Go ».</summary>
    public string SizeText { get; }

    /// <summary>Last modification.</summary>
    public string DateText { get; }

    /// <summary>« Dossier », « Vidéos · .mp4 ».</summary>
    public string TypeText { get; }

    /// <summary>Chosen for a deletion; never checked in advance.</summary>
    [ObservableProperty]
    private bool _isChecked;
}

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
    private readonly IManagerService _manager;
    private readonly IDialogService _dialogs;
    private readonly ISettingsService _settings;
    private string? _listed;
    private CancellationTokenSource? _listing;
    private readonly Dictionary<string, DiskMapNode> _details = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _loading = new(StringComparer.OrdinalIgnoreCase);
    private double _width;
    private double _height;

    /// <summary>Creates the ViewModel.</summary>
    public StorageMapViewModel(IEngineService engine, IStorageService storage, INavigationService navigation, OperationService operations, IManagerService manager, IDialogService dialogs, ISettingsService settings)
    {
        _manager = manager;
        _dialogs = dialogs;
        _settings = settings;
        _operations = operations;
        _engine = engine;
        _storage = storage;
        _navigation = navigation;
    }

    /// <summary>One bar per internal drive.</summary>
    public ObservableCollection<DriveBar> Drives { get; } = [];

    /// <summary>Rectangles of the level shown.</summary>
    public BulkObservableCollection<StorageTile> Tiles { get; } = [];

    /// <summary>Categories present in the level shown.</summary>
    public BulkObservableCollection<LegendItem> Legend { get; } = [];

    /// <summary>From the drive to the level shown.</summary>
    public ObservableCollection<BreadcrumbItem> Breadcrumb { get; } = [];

    /// <summary>The folder shown by the map, in a list under it (D83): its folders and files, largest first.</summary>
    public ObservableCollection<ExplorerRow> Entries { get; } = [];

    /// <summary>Folder of the explorer, « C:\Users\HP\Downloads ».</summary>
    [ObservableProperty]
    private string _folderText = string.Empty;

    /// <summary>« 3 dossier(s) · 12 fichier(s) · 40 Go ».</summary>
    [ObservableProperty]
    private string _explorerSummary = string.Empty;

    /// <summary>The folder is being read.</summary>
    [ObservableProperty]
    private bool _isListing;

    /// <summary>What the last deletion freed, or why nothing was done.</summary>
    [ObservableProperty]
    private string _explorerResult = string.Empty;

    /// <summary>The folder shown is an installed application: its uninstall is on the Applications page.</summary>
    [ObservableProperty]
    private bool _isApplicationFolder;

    /// <summary>The reading of the folder shown (tests wait for it).</summary>
    public Task ListingTask { get; private set; } = Task.CompletedTask;

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
    /// A click on a rectangle: leftovers open « Restes orphelins »; a folder opens, on the map
    /// and in the explorer under it (D83: application folders too, the page Applications is a
    /// link of the explorer). A level not computed yet gets its own map, with a loading sign in
    /// its rectangle while the engine is busy (D81, D82). Nothing is deleted from the map itself.
    /// </summary>
    public Task OpenAsync(StorageTile tile, CancellationToken cancellationToken = default) => OpenNodeAsync(tile.Node, cancellationToken);

    /// <summary>A click on a line of the explorer: a folder opens on both sides, a file is checked or unchecked.</summary>
    public Task OpenEntryAsync(ExplorerRow row, CancellationToken cancellationToken = default)
    {
        if (!row.IsFolder)
        {
            row.IsChecked = !row.IsChecked;
            return Task.CompletedTask;
        }

        DiskMapNode level = Breadcrumb[^1].Node;
        // A small folder gathered in « Autres petits éléments » is not on the map: its own map.
        DiskMapNode node = level.Children.FirstOrDefault(c => c.IsFolder && string.Equals(c.Path, row.Path, StringComparison.OrdinalIgnoreCase))
            ?? new DiskMapNode(row.Name, row.Path, row.SizeBytes, 0, level.Category, [], false, true);
        return OpenNodeAsync(node, cancellationToken);
    }

    private async Task OpenNodeAsync(DiskMapNode node, CancellationToken cancellationToken)
    {
        if (node.Category == StorageCategory.Leftovers)
        {
            _navigation.OpenOrphans(false);
            return;
        }

        if (node.IsGroup || !node.IsFolder || _loading.Contains(node.Path))
        {
            return;
        }

        if (node.Children.Any(c => c.IsFolder) || IsTerminal(node))
        {
            GoDown(node.Name, node);
            return;
        }

        // Sub-folders not on the map yet (below the levels of the map, or a map saved before
        // D81): a map of this folder alone. The user may go elsewhere meanwhile: the level is
        // entered only if the map still shows the level of the click.
        DiskMapNode clickedFrom = Breadcrumb[^1].Node;
        if (await DetailAsync(node, cancellationToken) is { } detail && ReferenceEquals(Breadcrumb[^1].Node, clickedFrom))
        {
            GoDown(node.Name, detail with { Name = node.Name });
        }
    }

    /// <summary>Only files below: known from the map, or from a level computed on its own.</summary>
    public bool IsTerminal(DiskMapNode node) =>
        node.IsFolder && (node.HasFolders == false || (_details.TryGetValue(node.Path, out DiskMapNode? detail) && detail.HasFolders == false));

    /// <summary>What a click does on <paramref name="node"/>, for its tooltip.</summary>
    public static string ClickHint(DiskMapNode node) => node switch
    {
        { Category: StorageCategory.Leftovers } => "Clic : ouvrir Restes orphelins",
        { IsGroup: true } => string.Empty,
        { IsFolder: false } => "Fichier : cochez-le dans la liste sous la carte pour le supprimer",
        _ => "Clic : ouvrir ce dossier, sur la carte et dans la liste dessous",
    };

    /// <summary>Checks every line of the explorer.</summary>
    [RelayCommand]
    private void CheckAll()
    {
        foreach (ExplorerRow row in Entries)
        {
            row.IsChecked = true;
        }
    }

    /// <summary>Unchecks every line of the explorer.</summary>
    [RelayCommand]
    private void UncheckAll()
    {
        foreach (ExplorerRow row in Entries)
        {
            row.IsChecked = false;
        }
    }

    /// <summary>Opens the folder shown in the Explorer of Windows.</summary>
    [RelayCommand]
    private void RevealFolder()
    {
        if (FolderText.Length > 0)
        {
            _manager.Reveal(FolderText);
        }
    }

    /// <summary>An application folder: its proper uninstall is on the Applications page.</summary>
    [RelayCommand]
    private void OpenApps() => _navigation.OpenApps();

    /// <summary>
    /// Deletes the checked lines after the confirmation of the Gestionnaire (D79): mode « Sûr »
    /// or « Immédiat », a sensitive zone only in expert mode with « CONFIRMER »; then says what
    /// was freed and reads the folder again.
    /// </summary>
    [RelayCommand]
    private async Task DeleteCheckedAsync(CancellationToken cancellationToken)
    {
        List<ExplorerRow> rows = Entries.Where(r => r.IsChecked).ToList();
        if (rows.Count == 0)
        {
            ExplorerResult = "Cochez d'abord les éléments à supprimer.";
            return;
        }

        ulong size = rows.Aggregate(0UL, (s, r) => s + r.SizeBytes);
        int sensitive = rows.Count(r => r.IsSensitive);
        if (sensitive > 0 && !_settings.ExpertMode)
        {
            ExplorerResult = $"{sensitive} élément(s) sont dans une zone sensible de Windows : activez le mode expert dans Réglages pour agir dessus.";
            return;
        }

        string[] details =
        [
            $"« Sûr » : restaurable ; vers la quarantaine de CleanSlate si elle est sur un autre disque ({Formats.Size(size)} libérés maintenant), sinon vers la Corbeille ({Formats.Size(size)} libérés quand elle sera vidée).",
            $"« Immédiat » : supprimé définitivement ; {Formats.Size(size)} libérés maintenant.",
        ];
        ManagerDecision decision = await _dialogs.ConfirmManagerAsync(new ManagerSummary($"Supprimer : {rows.Count} élément(s)", rows.Count, Formats.Size(size), sensitive, true, details));
        if (!decision.Confirmed)
        {
            return;
        }

        ulong Free() => _storage.Drives().Aggregate(0UL, (s, d) => s + d.FreeBytes);
        ulong before = Free();
        using (_operations.Begin($"Suppression de {rows.Count} élément(s)", "Storage"))
        {
            try
            {
                ActionResult result = await _storage.MeasureAsync(
                    StorageOperation.Manager,
                    _ => $"Supprimer depuis la carte : {rows.Count} élément(s), {Formats.Size(size)}",
                    () => _manager.DeleteAsync(rows.Select(r => r.Path).ToList(), decision.Mode, new ManualAction(_settings.ExpertMode, decision.Word), cancellationToken),
                    _ => 0UL);
                ulong after = Free();
                string freed = after > before ? $"{Formats.Size(after - before)} libérés maintenant" : "Aucune place libérée maintenant (Corbeille)";
                ItemError? error = result.Items.FirstOrDefault(i => i.Error is not null)?.Error;
                ExplorerResult = error is null
                    ? $"{freed}. La carte suivra au prochain « Actualiser »."
                    : $"{freed}. {result.Items.Count(i => i.Error is not null)} élément(s) non traités : {EngineErrors.Describe(error.Code, error.Path)}";
            }
            catch (EngineException ex)
            {
                ExplorerResult = ex.Message;
            }
        }

        _listed = null;
        RefreshListing();
        await ListingTask;
    }

    // The explorer follows the level of the map (D83): read when the level changes.
    private void RefreshListing()
    {
        if (Map is null || Breadcrumb.Count == 0)
        {
            return;
        }

        DiskMapNode level = Breadcrumb[^1].Node;
        if (!string.Equals(level.Path, _listed, StringComparison.OrdinalIgnoreCase))
        {
            _listed = level.Path;
            ListingTask = ListAsync(level);
        }
    }

    // Quick and read-only (never waits for an analysis); the sizes the map has are reused.
    private async Task ListAsync(DiskMapNode level)
    {
        _listing?.Cancel();
        var listing = new CancellationTokenSource();
        _listing = listing;
        IsListing = true;
        FolderText = level.Path;
        IsApplicationFolder = level.Category == StorageCategory.Applications
            && Path.GetFileName(Path.GetDirectoryName(level.Path.TrimEnd('\\')) ?? string.Empty) is "Program Files" or "Program Files (x86)" or "Programs";
        try
        {
            List<DiskMapNode> mapped = level.Children.Where(c => c.IsFolder && !c.IsGroup).ToList();
            IReadOnlyList<FolderEntry> entries = await _manager.ListFolderAsync(level.Path, mapped.Select(c => c.Path).ToList(), listing.Token);
            if (!ReferenceEquals(_listing, listing))
            {
                return; // another level was opened meanwhile
            }

            Dictionary<string, ulong> sizes = mapped.GroupBy(c => c.Path, StringComparer.OrdinalIgnoreCase).ToDictionary(g => g.Key, g => g.First().SizeBytes, StringComparer.OrdinalIgnoreCase);
            List<ExplorerRow> rows = entries
                .Select(e => new ExplorerRow(e, e.SizeBytes ?? sizes.GetValueOrDefault(e.Path)))
                .OrderByDescending(r => r.SizeBytes)
                .ThenBy(r => r.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();
            Entries.Clear();
            foreach (ExplorerRow row in rows)
            {
                Entries.Add(row);
            }

            ExplorerSummary = string.Format(French, "{0:N0} dossier(s) · {1:N0} fichier(s) · {2}, du plus gros au plus petit", rows.Count(r => r.IsFolder), rows.Count(r => !r.IsFolder), Formats.Size(rows.Aggregate(0UL, (t, r) => t + r.SizeBytes)));
        }
        catch (EngineException ex)
        {
            Entries.Clear();
            ExplorerSummary = ex.Message;
        }
        finally
        {
            if (ReferenceEquals(_listing, listing))
            {
                IsListing = false;
            }
        }
    }

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
            // A level: the last orphan scan is reused, never a new one first (D83, it took minutes).
            DiskMap map = await _engine.DiskMapAsync(new DiskMapRequest(node.Path, ScanLeftovers: false), progress, cancellationToken);
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

    // The rectangles and the legend are replaced at once: the page draws the map one time (D84).
    private void Layout()
    {
        if (Map is null || Breadcrumb.Count == 0)
        {
            Tiles.ReplaceAll([]);
            Legend.ReplaceAll([]);
            return;
        }

        var tiles = new List<StorageTile>();

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
                tiles.Add(new StorageTile(
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
        Tiles.ReplaceAll(tiles);
        Legend.ReplaceAll(CategoryTotals(level));
        RefreshListing();
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
