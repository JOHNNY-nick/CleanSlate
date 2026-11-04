using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Gestionnaire (D79, EF-22 to EF-35): confirmation, modes, sensitive zones, diagnosis.</summary>
public sealed class ManagerViewModelTests
{
    private const ulong Gb = 1024UL * 1024 * 1024;

    private static StorageAnalysis Analysis() => new(
        @"C:\",
        [
            new FileEntry(@"C:\Users\Ana\Videos\film.mp4", 4 * Gb, null, FileCategory.Videos, Zone.Normal),
            new FileEntry(@"C:\Windows\big.dll", 9 * Gb, null, FileCategory.System, Zone.Sensitive),
        ],
        [new CategoryTotal(FileCategory.Videos, 4 * Gb, 1, [])],
        [new FileEntry(@"C:\Users\Ana\Downloads\setup.exe", Gb, null, FileCategory.Installers, Zone.Normal)],
        [],
        [],
        3,
        14 * Gb,
        0,
        false);

    private static (ManagerViewModel Vm, IManagerService Manager, IDialogService Dialogs, ISettingsService Settings) Create(bool expert = false, ManagerDecision? decision = null, INavigationService? navigation = null)
    {
        var manager = Substitute.For<IManagerService>();
        manager.AnalyzeAsync(Arg.Any<string>(), Arg.Any<IProgress<ManagerProgress>?>(), Arg.Any<CancellationToken>()).Returns(Analysis());
        manager.CachesAsync(Arg.Any<CancellationToken>()).Returns(
            [new CacheItem("Temp", @"C:\Users\Ana\AppData\Local\Temp", 2 * Gb, 10, null, Zone.Normal), new CacheItem("Windows", @"C:\Windows\Temp", Gb, 3, null, Zone.Sensitive)]);
        manager.RecycleBinsAsync(Arg.Any<CancellationToken>()).Returns([new RecycleBinInfo("C:", Gb, 4)]);
        manager.SystemItemsAsync(Arg.Any<CancellationToken>()).Returns([new SystemItem(SystemTask.WindowsOld, 20 * Gb, true)]);
        manager.MeasuresAsync(Arg.Any<string>(), Arg.Any<CancellationToken>()).Returns([]);
        manager.DeleteAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<DeleteMode>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>())
            .Returns(new ActionResult([], 0, 0, false));
        manager.CleanCachesAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>()).Returns(new ActionResult([], 0, 0, false));
        manager.EmptyRecycleBinAsync(Arg.Any<string?>(), Arg.Any<CancellationToken>()).Returns(new ActionResult([], 0, 0, false));
        manager.RunSystemAsync(Arg.Any<SystemTask>(), Arg.Any<bool>(), Arg.Any<CancellationToken>()).Returns(new SystemOutcome(0, false, false, []));
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 100 * Gb, 6 * Gb)]);
        storage.Journal().Returns([]);
        // The journal measures around the action: the action itself must run.
        storage.MeasureAsync(Arg.Any<StorageOperation>(), Arg.Any<Func<ulong, string>>(), Arg.Any<Func<Task<ulong>>>(), Arg.Any<Func<ulong, ulong>>(), Arg.Any<Func<ulong, Guid?>?>())
            .Returns(call => call.ArgAt<Func<Task<ulong>>>(2)());
        var dialogs = Substitute.For<IDialogService>();
        dialogs.ConfirmManagerAsync(Arg.Any<ManagerSummary>()).Returns(decision ?? new ManagerDecision(true));
        var settings = OrphansViewModelTests.Settings();
        settings.ExpertMode.Returns(expert);
        var engine = Substitute.For<IEngineService>();
        engine.PurgeAsync(Arg.Any<PurgeRequest>(), Arg.Any<CancellationToken>()).Returns(new PurgeResult(0, false));
        var vm = new ManagerViewModel(manager, storage, dialogs, settings, engine, navigation ?? Substitute.For<INavigationService>(), new OperationService());
        return (vm, manager, dialogs, settings);
    }

    [Fact]
    public async Task TheDiagnosis_SaysHowFullEachDriveIs_AndSplitsSafeFromToCheck()
    {
        var (vm, _, _, _) = Create();
        await vm.LoadCommand.ExecuteAsync(null);

        Assert.StartsWith("C: est plein à 94 %", vm.DriveSentences[0]);
        RecoverableLine caches = vm.Recoverable.Single(r => r.Kind == RecoverableKind.Caches);
        Assert.Equal(2 * Gb, caches.SizeBytes); // the sensitive Windows cache is never in « sans risque »
        Assert.True(caches.Safe);
        Assert.False(vm.Recoverable.Single(r => r.Kind == RecoverableKind.WindowsOld).Safe);
        Assert.Equal(Formats.Size(3 * Gb), vm.SafeTotalText);
    }

    /// <summary>EF-22: a sensitive file is refused outside expert mode, before any dialog.</summary>
    [Fact]
    public async Task ASensitiveFile_NeedsTheExpertMode()
    {
        var (vm, manager, dialogs, _) = Create(expert: false);
        await vm.AnalyzeCommand.ExecuteAsync(null);
        vm.Largest.Single(r => r.IsSensitive).IsChecked = true;

        await vm.DeleteCheckedCommand.ExecuteAsync(null);

        await dialogs.DidNotReceive().ConfirmManagerAsync(Arg.Any<ManagerSummary>());
        await manager.DidNotReceive().DeleteAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<DeleteMode>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>());
        Assert.Contains("mode expert", vm.ResultText);
    }

    /// <summary>EF-23: the mode and the word chosen in the confirmation reach the engine.</summary>
    [Fact]
    public async Task TheModeAndTheWord_ChosenInTheConfirmation_ReachTheEngine()
    {
        var (vm, manager, dialogs, _) = Create(expert: true, decision: new ManagerDecision(true, DeleteMode.Immediate, "CONFIRMER"));
        await vm.AnalyzeCommand.ExecuteAsync(null);
        foreach (ManagerRow row in vm.Largest)
        {
            row.IsChecked = true;
        }

        await vm.DeleteCheckedCommand.ExecuteAsync(null);

        await dialogs.Received(1).ConfirmManagerAsync(Arg.Is<ManagerSummary>(s => s.ItemCount == 2 && s.SensitiveCount == 1 && s.OfferModes));
        await manager.Received(1).DeleteAsync(
            Arg.Is<IReadOnlyList<string>>(p => p.Count == 2),
            DeleteMode.Immediate,
            Arg.Is<ManualAction>(m => m.ExpertMode && m.Confirmation == "CONFIRMER"),
            Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task NothingIsDone_WhenTheConfirmationIsCancelled_AndNothingIsPrechecked()
    {
        var (vm, manager, _, _) = Create(decision: new ManagerDecision(false));
        await vm.AnalyzeCommand.ExecuteAsync(null);
        Assert.All(vm.Largest.Concat(vm.OldFiles), r => Assert.False(r.IsChecked));
        vm.OldFiles[0].IsChecked = true;

        await vm.DeleteCheckedCommand.ExecuteAsync(null);

        await manager.DidNotReceive().DeleteAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<DeleteMode>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>());
    }

    /// <summary>EF-29: « Tout libérer (sans risque) » cleans only normal caches, Recycle Bins, quarantine and updates.</summary>
    [Fact]
    public async Task FreeAllSafe_TouchesOnlyWhatIsSafe()
    {
        var (vm, manager, _, _) = Create();
        await vm.LoadCommand.ExecuteAsync(null);

        await vm.FreeAllSafeCommand.ExecuteAsync(null);

        await manager.Received().CleanCachesAsync(Arg.Is<IReadOnlyList<string>>(p => p.Count == 1 && p[0].EndsWith(@"Local\Temp")), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>());
        await manager.Received().EmptyRecycleBinAsync(null, Arg.Any<CancellationToken>());
        await manager.DidNotReceive().RunSystemAsync(SystemTask.WindowsOld, Arg.Any<bool>(), Arg.Any<CancellationToken>());
        await manager.DidNotReceive().DeleteAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<DeleteMode>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public void TheReport_ListsFilesInCsvAndHtml()
    {
        string csv = ManagerViewModel.ReportCsv(Analysis());
        Assert.StartsWith("Chemin;Taille (octets);Modifié;Catégorie;Zone", csv);
        Assert.Contains("\"C:\\Windows\\big.dll\";", csv);
        Assert.Contains("sensible", csv);
        string html = ManagerViewModel.ReportHtml(Analysis(), ["C: est plein à 94 %."]);
        Assert.Contains("<h2>Catégories</h2>", html);
        Assert.Contains(System.Net.WebUtility.HtmlEncode("Vidéos"), html);
    }

    /// <summary>D79: the actions of the Gestionnaire appear in the History, with the space freed.</summary>
    [Fact]
    public async Task TheHistory_ListsTheActionsOfTheGestionnaire()
    {
        var storage = Substitute.For<IStorageService>();
        storage.Journal().Returns(
        [
            new StorageEntry(DateTimeOffset.Now, StorageOperation.Manager, "C:", 10 * Gb, 12 * Gb, 0, "Supprimer : 3 élément(s), 2 Go"),
            new StorageEntry(DateTimeOffset.Now, StorageOperation.Purge, "C:", 10 * Gb, 11 * Gb, Gb, "30 jours"),
        ]);
        var engine = Substitute.For<IEngineService>();
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns([]);
        var history = new HistoryViewModel(engine, Substitute.For<IDialogService>(), Substitute.For<ISettingsService>(), Substitute.For<INavigationService>(), storage);

        await history.LoadCommand.ExecuteAsync(null);

        string line = Assert.Single(history.ManagerActions);
        Assert.Contains("Supprimer : 3 élément(s), 2 Go", line);
        Assert.EndsWith("C: +2 Go", line);
        Assert.True(history.HasManagerActions);
    }

    /// <summary>D82, D83: the type of a file is its category and its extension.</summary>
    [Fact]
    public void TheTypeOfAFile_IsItsCategoryAndItsExtension()
    {
        Assert.Equal("Vidéos · .mp4", ManagerViewModel.TypeText(@"C:\Users\Ana\Videos\Film.MP4", FileCategory.Videos));
        Assert.Equal(ManagerViewModel.CategoryName(FileCategory.Other), ManagerViewModel.TypeText(@"C:\Users\Ana\LISEZMOI", FileCategory.Other));
    }
}
