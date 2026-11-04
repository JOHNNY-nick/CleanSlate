using System.Collections.ObjectModel;
using System.Globalization;
using System.Text;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>A file or folder of a list of the Gestionnaire, selectable for an action.</summary>
public sealed partial class ManagerRow : ObservableObject
{
    /// <summary>Wraps a file found by an analysis.</summary>
    public ManagerRow(string path, ulong sizeBytes, DateTimeOffset? modified, Zone zone, string detail)
    {
        Path = path;
        SizeBytes = sizeBytes;
        SizeText = Formats.Size(sizeBytes);
        DateText = modified is { } m ? Formats.Date(DateOnly.FromDateTime(m.LocalDateTime)) : Formats.Unknown;
        Zone = zone;
        Detail = detail;
    }

    /// <summary>Full path.</summary>
    public string Path { get; }

    /// <summary>Bytes.</summary>
    public ulong SizeBytes { get; }

    /// <summary>« 1,2 Go ».</summary>
    public string SizeText { get; }

    /// <summary>Last modification.</summary>
    public string DateText { get; }

    /// <summary>The line under the path (type and date); hidden for a copy of the « Doublons » tab.</summary>
    public bool ShowDetail { get; init; } = true;

    /// <summary>Zone: sensitive items carry a badge and need the expert mode.</summary>
    public Zone Zone { get; }

    /// <summary>The sensitive badge is shown.</summary>
    public bool IsSensitive => Zone == Zone.Sensitive;

    /// <summary>Category, percent, or « copie à garder ».</summary>
    public string Detail { get; }

    /// <summary>Chosen for the next action; never pre-checked (EF-27).</summary>
    [ObservableProperty]
    private bool _isChecked;
}

/// <summary>A line of the « Diagnostic »: a category of space that can be freed (EF-29).</summary>
/// <param name="Kind">What it is.</param>
/// <param name="Name">« Caches », « Corbeilles »…</param>
/// <param name="SizeText">Size, or « — » when only the cleaning measures it.</param>
/// <param name="SizeBytes">Size in bytes (0 when unknown).</param>
/// <param name="Safe">« sans risque » (true) or « à vérifier ».</param>
/// <param name="Explanation">What it is and what freeing it changes.</param>
public sealed record RecoverableLine(RecoverableKind Kind, string Name, string SizeText, ulong SizeBytes, bool Safe, string Explanation);

/// <summary>Categories of the « Diagnostic ».</summary>
public enum RecoverableKind
{
    /// <summary>Regenerable caches outside the sensitive zones.</summary>
    Caches,

    /// <summary>Recycle Bins.</summary>
    RecycleBins,

    /// <summary>Quarantine past its delay.</summary>
    Quarantine,

    /// <summary>Windows Update and Delivery Optimization caches.</summary>
    UpdateCaches,

    /// <summary>Component store.</summary>
    ComponentStore,

    /// <summary>Old downloads.</summary>
    OldDownloads,

    /// <summary>Copies of duplicates.</summary>
    Duplicates,

    /// <summary>Previous installation of Windows.</summary>
    WindowsOld,

    /// <summary>Hibernation file.</summary>
    Hibernation,
}

/// <summary>A line of the « Système » tab.</summary>
/// <param name="Task">Clean-up.</param>
/// <param name="SizeText">Size, or « — ».</param>
/// <param name="Available">Something to clean.</param>
public sealed record SystemRow(SystemTask Task, string SizeText, bool Available);

/// <summary>A drive of the « Volumes » tab (display only, EF-34).</summary>
/// <param name="Root">« C:\ ».</param>
/// <param name="Text">« NTFS · 696 Go · 70 Go libres ».</param>
/// <param name="UsedPercent">0 to 100.</param>
public sealed record VolumeRow(string Root, string Text, int UsedPercent);

/// <summary>A measure of the « Évolution » tab.</summary>
/// <param name="DateText">« 4 nov. 2026 ».</param>
/// <param name="FreeText">Free space.</param>
/// <param name="FreePercent">0 to 100.</param>
public sealed record MeasureRow(string DateText, string FreeText, int FreePercent);

/// <summary>
/// « Gestionnaire » (D79): understand and free the space of the whole machine. Analyses are
/// read-only; every action goes through the confirmation (preview, mode « Sûr » or
/// « Immédiat », « CONFIRMER » for a sensitive zone in expert mode), then says how much was
/// freed now, measured on the drive before and after.
/// </summary>
public sealed partial class ManagerViewModel : ObservableObject
{
    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");
    private readonly IManagerService _manager;
    private readonly IStorageService _storage;
    private readonly IDialogService _dialogs;
    private readonly ISettingsService _settings;
    private readonly IEngineService _engine;
    private readonly INavigationService _navigation;
    private readonly OperationService _operations;
    private StorageAnalysis? _analysis;
    private IReadOnlyList<CacheItem> _caches = [];
    private IReadOnlyList<RecycleBinInfo> _bins = [];
    private IReadOnlyList<SystemItem> _system = [];
    private IReadOnlyList<DuplicateGroup> _duplicates = [];

    /// <summary>Creates the ViewModel.</summary>
    public ManagerViewModel(IManagerService manager, IStorageService storage, IDialogService dialogs, ISettingsService settings, IEngineService engine, INavigationService navigation, OperationService operations)
    {
        _operations = operations;
        _manager = manager;
        _storage = storage;
        _dialogs = dialogs;
        _settings = settings;
        _engine = engine;
        _navigation = navigation;
    }

    /// <summary>Tab shown: 0 Diagnostic, 1 Catégories, 2 Gros fichiers, 3 Fichiers anciens, 4 Doublons, 5 Système, 6 En attente, 7 Volumes.</summary>
    [ObservableProperty]
    private int _tabIndex;

    /// <summary>One sentence per drive, « C: est plein à 94 %. Causes principales : … ».</summary>
    public ObservableCollection<string> DriveSentences { get; } = [];

    /// <summary>The five largest consumers, explained.</summary>
    public ObservableCollection<string> TopConsumers { get; } = [];

    /// <summary>What can be freed, « sans risque » first.</summary>
    public ObservableCollection<RecoverableLine> Recoverable { get; } = [];

    /// <summary>« Tout libérer maintenant (sans risque) » : 4,2 Go.</summary>
    [ObservableProperty]
    private string _safeTotalText = Formats.Size(0);

    /// <summary>Largest files.</summary>
    public ObservableCollection<ManagerRow> Largest { get; } = [];

    /// <summary>Categories: « Vidéos · 120 Go · 340 fichiers ».</summary>
    public ObservableCollection<string> Categories { get; } = [];

    /// <summary>Files of the category chosen.</summary>
    public ObservableCollection<ManagerRow> CategoryFiles { get; } = [];

    /// <summary>Category chosen in the « Catégories » tab.</summary>
    [ObservableProperty]
    private int _categoryIndex = -1;

    /// <summary>Old files.</summary>
    public ObservableCollection<ManagerRow> OldFiles { get; } = [];

    /// <summary>Copies of duplicates (never the copy to keep).</summary>
    public ObservableCollection<ManagerRow> DuplicateCopies { get; } = [];

    /// <summary>The analysis of the drive runs: « Analyser » becomes « Analyse en cours… (Annuler) ».</summary>
    [ObservableProperty]
    private bool _isAnalyzing;

    // After an action: everything read again, the result of the action kept on screen.
    private Task ReloadAfterActionAsync()
    {
        _afterAction = true;
        return LoadCommand.ExecuteAsync(null);
    }

    /// <summary>Lines of the « Système » tab.</summary>
    public ObservableCollection<SystemRow> SystemRows { get; } = [];

    /// <summary>Recycle Bins: « C: · 1,2 Go · 30 élément(s) ».</summary>
    public ObservableCollection<string> RecycleBins { get; } = [];

    /// <summary>Deletions waiting for the next restart.</summary>
    public ObservableCollection<string> PendingAtRestart { get; } = [];

    /// <summary>Measures of C:, oldest first.</summary>
    public ObservableCollection<MeasureRow> Measures { get; } = [];

    /// <summary>What grew most since the previous measure.</summary>
    public ObservableCollection<string> Growth { get; } = [];

    /// <summary>Drives and partitions.</summary>
    public ObservableCollection<VolumeRow> Volumes { get; } = [];

    /// <summary>Physical disks: « Disque 0 · SSD · 1 To · 12 Go non alloués ».</summary>
    public ObservableCollection<string> Disks { get; } = [];

    /// <summary>An analysis or an action is running.</summary>
    [ObservableProperty]
    private bool _isBusy;

    /// <summary>« 120 000 fichiers · 80 Go ».</summary>
    [ObservableProperty]
    private string _progressText = string.Empty;

    /// <summary>« 2,1 Go libérés maintenant », or what failed.</summary>
    [ObservableProperty]
    private string _resultText = string.Empty;

    /// <summary>The drive has been analyzed.</summary>
    [ObservableProperty]
    private bool _hasAnalysis;

    /// <summary>Folder or drive analyzed: the system drive by default.</summary>
    public string AnalysisRoot { get; set; } = (Path.GetPathRoot(Environment.SystemDirectory) ?? @"C:\").ToUpperInvariant();

    /// <summary>Expert mode (Réglages).</summary>
    public bool ExpertMode => _settings.ExpertMode;

    /// <summary>A page opened again within this time reuses what was read (D84).</summary>
    public static readonly TimeSpan ReloadAfter = TimeSpan.FromMinutes(1);

    private DateTimeOffset _loadedAt = DateTimeOffset.MinValue;
    private bool _afterAction;

    /// <summary>
    /// Reads what is quick (drives, caches, Recycle Bins, system, measures) and records a
    /// measure. Opened again within <see cref="ReloadAfter"/>, the page keeps what it read
    /// (D84: the sizes of Windows.old or of the Windows Update cache take seconds); after an
    /// action everything is read again, and the result of the action stays shown.
    /// </summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        bool afterAction = _afterAction;
        _afterAction = false;
        if (!afterAction)
        {
            if (DateTimeOffset.Now - _loadedAt < ReloadAfter)
            {
                return;
            }

            ResultText = string.Empty;
        }

        _loadedAt = DateTimeOffset.Now;
        try
        {
            _caches = await _manager.CachesAsync(cancellationToken);
            _bins = await Optional(() => _manager.RecycleBinsAsync(cancellationToken));
            _system = await Optional(() => _manager.SystemItemsAsync(cancellationToken));
            IReadOnlyList<string> pending = await Optional(() => _manager.PendingAtRestartAsync(cancellationToken));
            IReadOnlyList<VolumeInfo> volumes = await Optional(() => _manager.VolumesAsync(cancellationToken));
            Fill(PendingAtRestart, pending);
            Fill(RecycleBins, _bins.Select(b => $"{b.Drive} · {Formats.Size(b.SizeBytes)} · {b.Items} élément(s)"));
            Fill(SystemRows, _system.Select(s => new SystemRow(s.Task, s.SizeBytes is { } b ? Formats.Size(b) : Formats.Unknown, s.Available)));
            Fill(Volumes, volumes.Select(v => new VolumeRow(
                v.Root,
                $"{(v.Label.Length > 0 ? v.Label + " · " : string.Empty)}{v.FileSystem} · {Formats.Size(v.TotalBytes)} · {Formats.Size(v.FreeBytes)} libres",
                v.TotalBytes == 0 ? 0 : (int)Math.Round(100.0 * (v.TotalBytes - v.FreeBytes) / v.TotalBytes))));
            IReadOnlyList<DiskInfo> disks = await Optional(() => _manager.DisksAsync(cancellationToken));
            Fill(Disks, disks.Select(d => $"Disque {d.Number} · {(d.Name.Length > 0 ? d.Name + " · " : string.Empty)}{Formats.Size(d.SizeBytes)} · {Formats.Size(d.UnallocatedBytes)} non alloués"));
            await RecordAndShowMeasuresAsync(cancellationToken);
        }
        catch (EngineException ex)
        {
            ResultText = ex.Message;
        }

        Diagnose();

    }

    /// <summary>Walks C: (read-only), with progress and cancel.</summary>
    [RelayCommand(IncludeCancelCommand = true)]
    private async Task AnalyzeAsync(CancellationToken cancellationToken)
    {
        IsBusy = true;
        IsAnalyzing = true;
        using OperationService.Operation operation = _operations.Begin($"Analyse de {AnalysisRoot}", "Manager", () => AnalyzeCancelCommand.Execute(null));
        try
        {
            var progress = new Progress<ManagerProgress>(p =>
            {
                ProgressText = string.Format(French, "{0:N0} fichiers · {1}", p.Files, Formats.Size(p.Bytes));
                operation.Report(ProgressText);
            });
            _analysis = await _manager.AnalyzeAsync(AnalysisRoot, progress, cancellationToken);
            HasAnalysis = true;
            Fill(Largest, _analysis.Largest.Select(Row));
            Fill(Categories, _analysis.Categories.Select(c => string.Format(French, "{0} · {1} · {2:N0} fichier(s)", CategoryName(c.Category), Formats.Size(c.SizeBytes), c.Files)));
            Fill(OldFiles, _analysis.OldFiles.Select(Row));
            ProgressText = string.Format(French, "{0:N0} fichiers · {1}{2}", _analysis.Files, Formats.Size(_analysis.SizeBytes), _analysis.Denied > 0 ? $" · {_analysis.Denied} dossier(s) inaccessible(s) ignoré(s)" : string.Empty);
            Diagnose();
        }
        catch (EngineException ex)
        {
            ResultText = ex.Message;
        }
        finally
        {
            IsBusy = false;
            IsAnalyzing = false;
        }
    }

    /// <summary>Looks for duplicates among the candidates of the analysis (SHA-256).</summary>
    [RelayCommand]
    private async Task FindDuplicatesAsync(CancellationToken cancellationToken)
    {
        if (_analysis is null)
        {
            return;
        }

        IsBusy = true;
        using IDisposable operation = _operations.Begin("Recherche des doublons", "Manager");
        try
        {
            _duplicates = await _manager.DuplicatesAsync(_analysis.DuplicateCandidates, cancellationToken);
            Fill(DuplicateCopies, _duplicates.SelectMany(g => g.Files
                .Where(f => f.Path != g.Keep)
                .Select(f => new ManagerRow(f.Path, f.SizeBytes, f.Modified, f.Zone, $"copie de {g.Keep}") { ShowDetail = false })));
            Diagnose();
        }
        catch (EngineException ex)
        {
            ResultText = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }
    }

    partial void OnCategoryIndexChanged(int value)
    {
        CategoryFiles.Clear();
        if (_analysis is not null && value >= 0 && value < _analysis.Categories.Count)
        {
            foreach (FileEntry f in _analysis.Categories[value].Largest)
            {
                CategoryFiles.Add(Row(f));
            }
        }
    }

    /// <summary>Deletes the checked items of every list, in the mode chosen in the confirmation.</summary>
    [RelayCommand]
    private Task DeleteCheckedAsync(CancellationToken cancellationToken) =>
        ActOnCheckedAsync("Supprimer", offerModes: true, cancellationToken, (paths, decision, manual) => _manager.DeleteAsync(paths, decision.Mode, manual, cancellationToken));

    /// <summary>Opens the first checked item in the Explorer.</summary>
    [RelayCommand]
    private void Reveal() => Each(_manager.Reveal);

    /// <summary>An application: its full uninstall through the wizard.</summary>
    [RelayCommand]
    private void Uninstall() => _navigation.OpenApps();

    /// <summary>Opens the storage map (« Carte »).</summary>
    [RelayCommand]
    private void OpenMap() => _navigation.OpenStorage();

    /// <summary>Frees one category of the « Diagnostic ».</summary>
    [RelayCommand]
    private async Task FreeAsync(RecoverableLine? line)
    {
        if (line is null)
        {
            return;
        }

        switch (line.Kind)
        {
            case RecoverableKind.OldDownloads:
                TabIndex = 3;
                return;
            case RecoverableKind.Duplicates:
                TabIndex = 4;
                return;
        }

        var summary = new ManagerSummary($"Libérer : {line.Name}", 1, line.SizeText, 0, false, [line.Explanation]);
        if (!(await _dialogs.ConfirmManagerAsync(summary)).Confirmed)
        {
            return;
        }

        await MeasuredAsync($"Libérer : {line.Name}", () => FreeKindAsync(line.Kind, CancellationToken.None));
    }

    /// <summary>« Tout libérer maintenant (sans risque) » : caches, Corbeilles, quarantaine à purger, mises à jour (EF-29).</summary>
    [RelayCommand]
    private async Task FreeAllSafeAsync()
    {
        List<RecoverableLine> safe = Recoverable.Where(r => r.Safe).ToList();
        var summary = new ManagerSummary(
            $"Tout libérer maintenant (sans risque) : {SafeTotalText}",
            safe.Count,
            SafeTotalText,
            0,
            false,
            safe.Select(r => $"{r.Name} : {r.SizeText}").Append("Rien n'est pris dans vos fichiers personnels ni dans les zones sensibles de Windows.").ToList());
        if (!(await _dialogs.ConfirmManagerAsync(summary)).Confirmed)
        {
            return;
        }

        await MeasuredAsync("Tout libérer (sans risque)", async () =>
        {
            foreach (RecoverableLine line in safe)
            {
                await FreeKindAsync(line.Kind, CancellationToken.None);
            }
        });
    }

    /// <summary>Runs a clean-up of the « Système » tab after its confirmation.</summary>
    [RelayCommand]
    private async Task RunSystemAsync(SystemRow? row)
    {
        if (row is null)
        {
            return;
        }

        var summary = new ManagerSummary($"Nettoyage système : {SystemName(row.Task)}", 1, row.SizeText, 0, false, [SystemExplanation(row.Task)]);
        if (!(await _dialogs.ConfirmManagerAsync(summary)).Confirmed)
        {
            return;
        }

        await MeasuredAsync($"Nettoyage système : {SystemName(row.Task)}", async () =>
        {
            SystemOutcome outcome = await _manager.RunSystemAsync(row.Task, _settings.ExpertMode, CancellationToken.None);
            if (outcome.RestartNeeded)
            {
                ResultText = "Windows demande un redémarrage pour terminer.";
            }
        });
    }

    /// <summary>Cancels a deletion planned for the next restart (EF-28).</summary>
    [RelayCommand]
    private async Task CancelAtRestartAsync(string? path)
    {
        if (path is null)
        {
            return;
        }

        try
        {
            await _manager.CancelAtRestartAsync(path, CancellationToken.None);
            ResultText = $"Suppression au redémarrage annulée : {path}";
        }
        catch (EngineException ex)
        {
            ResultText = ex.Message;
        }

        await ReloadAfterActionAsync();
    }

    /// <summary>Empties every Recycle Bin.</summary>
    [RelayCommand]
    private async Task EmptyRecycleBinsAsync()
    {
        ulong size = _bins.Aggregate(0UL, (s, b) => s + b.SizeBytes);
        var summary = new ManagerSummary("Vider les Corbeilles de tous les lecteurs", _bins.Count, Formats.Size(size), 0, false, ["Rien de ce qu'elles contiennent ne pourra être restauré."]);
        if ((await _dialogs.ConfirmManagerAsync(summary)).Confirmed)
        {
            await MeasuredAsync("Vider les Corbeilles", () => _manager.EmptyRecycleBinAsync(null, CancellationToken.None));
        }
    }

    /// <summary>
    /// « Exporter le rapport » (D85): a PDF in the style of the book and the CSV table, in the
    /// export folder (else the Desktop), then the folder is shown. Without an analysis, says so:
    /// the button used to do nothing.
    /// </summary>
    [RelayCommand]
    private async Task ExportReportAsync(CancellationToken cancellationToken)
    {
        if (_analysis is null)
        {
            ResultText = "Lancez d'abord « Analyser le disque » : le rapport décrit le résultat de l'analyse.";
            return;
        }

        string stamp = DateTime.Now.ToString("yyyyMMdd-HHmm", French);
        using IDisposable operation = _operations.Begin("Création du rapport PDF", "Manager");
        try
        {
            string csv = _manager.SaveReport($"CleanSlate-analyse-{stamp}.csv", ReportCsv(_analysis));
            string pdf = await _manager.SavePdfAsync($"CleanSlate-rapport-{stamp}.pdf", StorageReport.Html(_analysis, DriveSentences, Recoverable, DateTimeOffset.Now), cancellationToken);
            ResultText = $"Rapport enregistré : {pdf} (et le tableau {Path.GetFileName(csv)})";
            _manager.Reveal(pdf);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            ResultText = $"Le rapport n'a pas pu être enregistré : {ex.Message}";
        }
    }

    /// <summary>CSV of the analysis: one line per file of the largest and of each category.</summary>
    public static string ReportCsv(StorageAnalysis analysis)
    {
        var text = new StringBuilder("Chemin;Taille (octets);Modifié;Catégorie;Zone\r\n");
        foreach (FileEntry f in analysis.Largest.Concat(analysis.Categories.SelectMany(c => c.Largest)).DistinctBy(f => f.Path))
        {
            text.Append('"').Append(f.Path.Replace("\"", "\"\"")).Append("\";")
                .Append(f.SizeBytes.ToString(CultureInfo.InvariantCulture)).Append(';')
                .Append(f.Modified?.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture)).Append(';')
                .Append(CategoryName(f.Category)).Append(';')
                .Append(f.Zone == Zone.Sensitive ? "sensible" : "normale").Append("\r\n");
        }

        return text.ToString();
    }

    /// <summary>French name of a category.</summary>
    public static string CategoryName(FileCategory category) => category switch
    {
        FileCategory.Videos => "Vidéos",
        FileCategory.Photos => "Photos",
        FileCategory.Music => "Musique",
        FileCategory.Documents => "Documents",
        FileCategory.Games => "Jeux",
        FileCategory.DevTools => "Outils de développement",
        FileCategory.VirtualMachines => "Machines virtuelles",
        FileCategory.PhoneBackups => "Sauvegardes de téléphone",
        FileCategory.Installers => "Installeurs téléchargés",
        FileCategory.Archives => "Archives",
        FileCategory.Applications => "Applications",
        FileCategory.System => "Windows et système",
        FileCategory.Caches => "Caches",
        _ => "Autres",
    };

    /// <summary>French name of a clean-up of Windows.</summary>
    public static string SystemName(SystemTask task) => task switch
    {
        SystemTask.ComponentStore => "Magasin de composants (WinSxS)",
        SystemTask.UpdateCache => "Cache de Windows Update",
        SystemTask.DeliveryOptimization => "Cache de l'Optimisation de la distribution",
        SystemTask.WindowsOld => "Ancienne installation de Windows (Windows.old)",
        SystemTask.Hibernation => "Fichier d'hibernation",
        _ => "Anciens points de restauration",
    };

    /// <summary>What a clean-up of Windows does and changes.</summary>
    public static string SystemExplanation(SystemTask task) => task switch
    {
        SystemTask.ComponentStore => "Windows retire, par DISM, les anciennes versions de ses composants remplacées par les mises à jour. Les mises à jour déjà installées ne pourront plus être désinstallées.",
        SystemTask.UpdateCache => "Les mises à jour déjà téléchargées sont effacées ; les services de Windows Update sont arrêtés puis redémarrés proprement. Windows les retélécharge si besoin.",
        SystemTask.DeliveryOptimization => "Fichiers que Windows partage avec les autres PC pour accélérer les mises à jour ; ils se recréent.",
        SystemTask.WindowsOld => "Copie de la version précédente de Windows, gardée après une mise à niveau : supprimée par le Nettoyage de disque de Windows, le retour à la version précédente ne sera plus possible.",
        SystemTask.Hibernation => "La mise en veille prolongée et le démarrage rapide sont désactivés (powercfg) : le fichier hiberfil.sys disparaît.",
        _ => "Les anciens points de restauration sont supprimés ; le plus récent et ceux de moins de 30 jours sont gardés (sauf en mode expert).",
    };

    // The diagnosis: one sentence per drive, the five largest consumers, what can be freed.
    private void Diagnose()
    {
        DriveSentences.Clear();
        TopConsumers.Clear();
        foreach (DriveSpace drive in _storage.Drives())
        {
            int used = drive.TotalBytes == 0 ? 0 : (int)Math.Round(100.0 * (drive.TotalBytes - drive.FreeBytes) / drive.TotalBytes);
            string causes = string.Empty;
            if (_storage.CachedMap(drive.Name) is { } cached)
            {
                causes = " Causes principales : " + string.Join(", ", cached.Map.Root.Children.Where(c => !c.IsGroup).OrderByDescending(c => c.SizeBytes).Take(3)
                    .Select(c => $"{c.Name} ({Formats.Size(c.SizeBytes)})")) + ".";
                foreach (DiskMapNode node in cached.Map.Root.Children.SelectMany(c => c.Children.Count > 0 ? c.Children : [c])
                             .Where(n => !n.IsGroup).OrderByDescending(n => n.SizeBytes).Take(5))
                {
                    TopConsumers.Add($"{node.Path} · {Formats.Size(node.SizeBytes)} — {Explain(node.Category)}");
                }
            }

            DriveSentences.Add($"{drive.Name} est plein à {used} % ({Formats.Size(drive.FreeBytes)} libres).{causes}");
        }

        Recoverable.Clear();
        ulong caches = _caches.Where(c => c.Zone == Zone.Normal).Aggregate(0UL, (s, c) => s + c.SizeBytes);
        ulong bins = _bins.Aggregate(0UL, (s, b) => s + b.SizeBytes);
        ulong updates = _system.Where(s => s.Task is SystemTask.UpdateCache or SystemTask.DeliveryOptimization).Aggregate(0UL, (s, i) => s + (i.SizeBytes ?? 0));
        Add(RecoverableKind.Caches, "Caches et fichiers temporaires", caches, true, "Fichiers que les applications et les navigateurs recréent d'eux-mêmes ; jamais un fichier ouvert.");
        Add(RecoverableKind.RecycleBins, "Corbeilles", bins, true, "Ce que vous avez déjà supprimé ; plus restaurable ensuite.");
        Add(RecoverableKind.Quarantine, "Quarantaine à purger", null, true, $"Sessions de plus de {_settings.QuarantineDays} jours, déjà au-delà du délai de restauration.");
        Add(RecoverableKind.UpdateCaches, "Caches des mises à jour", updates, true, "Mises à jour déjà installées ; Windows les retélécharge si besoin.");
        Add(RecoverableKind.ComponentStore, "Magasin de composants", null, true, SystemExplanation(SystemTask.ComponentStore));
        if (_analysis is not null)
        {
            ulong downloads = _analysis.OldFiles.Where(f => f.Path.Contains(@"\Downloads\", StringComparison.OrdinalIgnoreCase)).Aggregate(0UL, (s, f) => s + f.SizeBytes);
            Add(RecoverableKind.OldDownloads, "Anciens téléchargements", downloads, false, "Téléchargements non ouverts depuis plus de 90 jours : à relire avant de supprimer.");
        }

        if (_duplicates.Count > 0)
        {
            ulong copies = _duplicates.Aggregate(0UL, (s, g) => s + (g.SizeBytes * (ulong)(g.Files.Count - 1)));
            Add(RecoverableKind.Duplicates, "Copies en double", copies, false, "Fichiers identiques ; une copie de chaque est toujours gardée.");
        }

        foreach (SystemItem item in _system.Where(s => s.Task is SystemTask.WindowsOld or SystemTask.Hibernation && s.Available))
        {
            Add(item.Task == SystemTask.WindowsOld ? RecoverableKind.WindowsOld : RecoverableKind.Hibernation, SystemName(item.Task), item.SizeBytes, false, SystemExplanation(item.Task));
        }

        SafeTotalText = Formats.Size(Recoverable.Where(r => r.Safe).Aggregate(0UL, (s, r) => s + r.SizeBytes));
    }

    private void Add(RecoverableKind kind, string name, ulong? size, bool safe, string explanation) =>
        Recoverable.Add(new RecoverableLine(kind, name, size is { } s ? Formats.Size(s) : Formats.Unknown, size ?? 0, safe, explanation));

    private static string Explain(StorageCategory category) => category switch
    {
        StorageCategory.Applications => "programmes installés",
        StorageCategory.UserFiles => "vos fichiers personnels",
        StorageCategory.System => "Windows et ses fichiers",
        StorageCategory.Leftovers => "restes d'applications désinstallées, récupérables",
        StorageCategory.Quarantine => "quarantaine de CleanSlate, libérée à la purge",
        _ => "données d'applications et autres",
    };

    private async Task FreeKindAsync(RecoverableKind kind, CancellationToken cancellationToken)
    {
        switch (kind)
        {
            case RecoverableKind.Caches:
                await _manager.CleanCachesAsync(_caches.Where(c => c.Zone == Zone.Normal).Select(c => c.Path).ToList(), new ManualAction(), cancellationToken);
                break;
            case RecoverableKind.RecycleBins:
                await _manager.EmptyRecycleBinAsync(null, cancellationToken);
                break;
            case RecoverableKind.Quarantine:
                await _engine.PurgeAsync(new PurgeRequest((uint)_settings.QuarantineDays, DryRun: false), cancellationToken);
                break;
            case RecoverableKind.UpdateCaches:
                await _manager.RunSystemAsync(SystemTask.UpdateCache, false, cancellationToken);
                await _manager.RunSystemAsync(SystemTask.DeliveryOptimization, false, cancellationToken);
                break;
            case RecoverableKind.ComponentStore:
                await _manager.RunSystemAsync(SystemTask.ComponentStore, false, cancellationToken);
                break;
            case RecoverableKind.WindowsOld:
                await _manager.RunSystemAsync(SystemTask.WindowsOld, false, cancellationToken);
                break;
            case RecoverableKind.Hibernation:
                await _manager.RunSystemAsync(SystemTask.Hibernation, false, cancellationToken);
                break;
        }
    }

    // Asks, acts on every checked row, then tells the space freed now.
    private async Task ActOnCheckedAsync(string verb, bool offerModes, CancellationToken cancellationToken, Func<IReadOnlyList<string>, ManagerDecision, ManualAction, Task<ActionResult>> act, string? warning = null)
    {
        List<ManagerRow> rows = Checked().ToList();
        if (rows.Count == 0)
        {
            return;
        }

        ulong size = rows.Aggregate(0UL, (s, r) => s + r.SizeBytes);
        int sensitive = rows.Count(r => r.IsSensitive);
        if (sensitive > 0 && !_settings.ExpertMode)
        {
            ResultText = $"{sensitive} élément(s) sont dans une zone sensible de Windows : activez le mode expert dans Réglages pour agir dessus.";
            return;
        }

        var details = new List<string>();
        if (offerModes)
        {
            details.Add($"« Sûr » : restaurable ; vers la quarantaine de CleanSlate si elle est sur un autre disque ({Formats.Size(size)} libérés maintenant, restauration depuis l'Historique), sinon vers la Corbeille ({Formats.Size(size)} libérés quand elle sera vidée).");
            details.Add($"« Immédiat » : supprimé définitivement ; {Formats.Size(size)} libérés maintenant.");
        }

        if (warning is not null)
        {
            details.Add(warning);
        }

        ManagerDecision decision = await _dialogs.ConfirmManagerAsync(new ManagerSummary($"{verb} : {rows.Count} élément(s)", rows.Count, Formats.Size(size), sensitive, offerModes, details));
        if (!decision.Confirmed)
        {
            return;
        }

        var manual = new ManualAction(_settings.ExpertMode, decision.Word);
        await MeasuredAsync($"{verb} : {rows.Count} élément(s), {Formats.Size(size)}", async () =>
        {
            ActionResult result = await act(rows.Select(r => r.Path).ToList(), decision, manual);
            int failed = result.Items.Count(i => i.Error is not null);
            if (failed > 0)
            {
                ItemError error = result.Items.First(i => i.Error is not null).Error!;
                ResultText = $"{failed} élément(s) non traités : {EngineErrors.Describe(error.Code, error.Path)}";
            }

            foreach (ManagerRow row in rows.Where(r => result.Items.Any(i => i.Path == r.Path && i.Error is null)))
            {
                Largest.Remove(row);
                OldFiles.Remove(row);
                CategoryFiles.Remove(row);
                DuplicateCopies.Remove(row);
            }
        });
    }

    // Measures the free space before and after: « 2,1 Go libérés maintenant » (EF-35); the
    // action is written in the journal, shown in the History.
    private async Task MeasuredAsync(string what, Func<Task> action)
    {
        ulong Free() => _storage.Drives().Aggregate(0UL, (s, d) => s + d.FreeBytes);
        ulong before = Free();
        IsBusy = true;
        ResultText = string.Empty;
        using IDisposable operation = _operations.Begin(what, "Manager");
        try
        {
            await _storage.MeasureAsync(StorageOperation.Manager, _ => what, async () =>
            {
                await action();
                return 0UL;
            }, _ => 0UL);
            ulong after = Free();
            string freed = after > before ? $"{Formats.Size(after - before)} libérés maintenant" : "Aucune place libérée maintenant (Corbeille ou simulation)";
            ResultText = ResultText.Length > 0 ? $"{freed}. {ResultText}" : freed;
        }
        catch (EngineException ex)
        {
            ResultText = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }

        await ReloadAfterActionAsync();
    }

    private async Task RecordAndShowMeasuresAsync(CancellationToken cancellationToken)
    {
        string system = Path.GetPathRoot(Environment.SystemDirectory)?.TrimEnd('\\') ?? "C:";
        if (_storage.Drives().FirstOrDefault(d => string.Equals(d.Name, system, StringComparison.OrdinalIgnoreCase)) is { } drive)
        {
            List<FolderSize> largest = _storage.CachedMap(drive.Name)?.Map.Root.Children
                .Where(c => !c.IsGroup).Select(c => new FolderSize(c.Path, c.SizeBytes)).ToList() ?? [];
            await _manager.RecordMeasureAsync(new DriveMeasure(DateTimeOffset.Now, drive.Name, drive.TotalBytes, drive.FreeBytes, largest), cancellationToken);
        }

        IReadOnlyList<DriveMeasure> measures = await _manager.MeasuresAsync(system, cancellationToken);
        Fill(Measures, measures.TakeLast(60).Select(m => new MeasureRow(
            $"{Formats.Date(DateOnly.FromDateTime(m.TakenAt.LocalDateTime))} {m.TakenAt.LocalDateTime:HH:mm}",
            $"{Formats.Size(m.FreeBytes)} libres",
            m.TotalBytes == 0 ? 0 : (int)Math.Round(100.0 * m.FreeBytes / m.TotalBytes))));
        Growth.Clear();
        if (measures.Count >= 2)
        {
            Dictionary<string, ulong> before = measures[^2].Largest.ToDictionary(f => f.Path, f => f.Bytes, StringComparer.OrdinalIgnoreCase);
            foreach (FolderSize f in measures[^1].Largest.Where(f => f.Bytes > before.GetValueOrDefault(f.Path)).OrderByDescending(f => f.Bytes - before.GetValueOrDefault(f.Path)).Take(5))
            {
                Growth.Add($"{f.Path} : +{Formats.Size(f.Bytes - before.GetValueOrDefault(f.Path))}");
            }
        }
    }

    private IEnumerable<ManagerRow> Checked() =>
        Largest.Concat(OldFiles).Concat(CategoryFiles).Concat(DuplicateCopies).Where(r => r.IsChecked).DistinctBy(r => r.Path);

    private void Each(Action<string> action)
    {
        if (Checked().FirstOrDefault() is { } row)
        {
            action(row.Path);
        }
    }

    private static ManagerRow Row(FileEntry f) => new(f.Path, f.SizeBytes, f.Modified, f.Zone, CategoryName(f.Category));

    /// <summary>Type of a file: its category and its extension, « Vidéos · .mp4 » (D82).</summary>
    public static string TypeText(string path, FileCategory category)
    {
        string extension = Path.GetExtension(path);
        return extension.Length > 0 ? $"{CategoryName(category)} · {extension.ToLowerInvariant()}" : CategoryName(category);
    }

    private static async Task<IReadOnlyList<T>> Optional<T>(Func<Task<IReadOnlyList<T>>> read)
    {
        try
        {
            return await read();
        }
        catch (EngineException)
        {
            return [];
        }
    }

    private static void Fill<T>(ObservableCollection<T> list, IEnumerable<T> items)
    {
        list.Clear();
        foreach (T item in items)
        {
            list.Add(item);
        }
    }
}
