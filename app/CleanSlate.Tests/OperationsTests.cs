using System.Text.RegularExpressions;
using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// D82, corrections after the first use of the Gestionnaire: long operations shown on every
/// page, requests queued instead of « BUSY », French errors, no console window.
/// </summary>
public sealed partial class OperationsTests
{
    private const ulong Gb = 1024UL * 1024 * 1024;

    private static string Repository()
    {
        for (DirectoryInfo? dir = new(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            if (Directory.Exists(Path.Combine(dir.FullName, "core")) && Directory.Exists(Path.Combine(dir.FullName, "app")))
            {
                return dir.FullName;
            }
        }

        throw new DirectoryNotFoundException("racine du dépôt introuvable");
    }

    [Fact]
    public void TheIndicator_ShowsTheOperation_ItsProgress_AndWhatWaits()
    {
        var operations = new OperationService();
        bool cancelled = false;
        Assert.False(operations.IsActive);

        OperationService.Operation analysis = operations.Begin("Analyse de C:\\", "Manager", () => cancelled = true);
        analysis.Report("120 000 fichiers · 80 Go");
        Assert.True(operations.IsActive);
        Assert.Equal("Manager", operations.PageTag);
        Assert.Equal("Analyse de C:\\ · 120 000 fichiers · 80 Go", operations.Text);

        IDisposable waiting = operations.Wait();
        Assert.EndsWith("1 demande en attente", operations.Text);
        operations.Cancel();
        Assert.True(cancelled);

        waiting.Dispose();
        analysis.Dispose();
        Assert.False(operations.IsActive);
        Assert.Equal(string.Empty, operations.Text);
    }

    /// <summary>« Busy, the engine is busy with another operation »: a second request now waits its turn.</summary>
    [Fact]
    public async Task ASecondRequest_WaitsForTheEngine_InsteadOfFailing()
    {
        var operations = new OperationService();
        var turn = new EngineTurn(operations);
        var first = new TaskCompletionSource<int>();
        Task<int> running = turn.RunAsync(() => first.Task, TestContext.Current.CancellationToken);
        bool secondStarted = false;
        Task<int> queued = turn.RunAsync(() => { secondStarted = true; return Task.FromResult(2); }, TestContext.Current.CancellationToken);

        Assert.False(secondStarted);
        Assert.True(operations.IsActive, "the wait is shown");

        first.SetResult(1);
        Assert.Equal(1, await running);
        Assert.Equal(2, await queued);
        Assert.True(secondStarted);
        Assert.False(operations.IsActive);
    }

    [Fact]
    public async Task AWaitCancelled_EndsAsCancelled_InFrench()
    {
        var turn = new EngineTurn(new OperationService());
        var first = new TaskCompletionSource<int>();
        Task<int> running = turn.RunAsync(() => first.Task, TestContext.Current.CancellationToken);
        using var cancel = new CancellationTokenSource();
        Task<int> queued = turn.RunAsync(() => Task.FromResult(2), cancel.Token);

        cancel.Cancel();
        EngineException ex = await Assert.ThrowsAsync<EngineException>(() => queued);
        Assert.Equal("CANCELLED", ex.Code);
        Assert.Equal("Opération annulée.", ex.Message);
        first.SetResult(1);
        await running;
    }

    /// <summary>Every code the engine can send has its French text: never an English message.</summary>
    [Fact]
    public void EveryErrorCodeOfTheEngine_HasAFrenchText()
    {
        string root = Repository();
        string sources = File.ReadAllText(Path.Combine(root, "core", "cleanslate-core", "src", "error.rs"))
            + File.ReadAllText(Path.Combine(root, "core", "cleanslate-ffi", "src", "lib.rs"));
        string[] codes = ErrorCode().Matches(sources).Select(m => m.Groups[1].Value).Distinct().ToArray();
        Assert.Contains("BUSY", codes);
        Assert.All(codes, code => Assert.True(EngineErrors.IsKnown(code), $"{code} sans texte français"));

        var busy = new EngineException("BUSY", "the engine is busy with another operation");
        Assert.Equal("Une analyse est déjà en cours. Attendez sa fin ou annulez-la.", busy.Message);
        Assert.Contains("busy", busy.Detail);
        Assert.Contains(@"C:\x", new EngineException("ACCESS_DENIED", "denied", @"C:\x").Message);
        Assert.DoesNotContain("engine", new EngineException("SOMETHING_NEW", "the engine said no").Message);
    }

    [GeneratedRegex("\"([A-Z][A-Z_]{3,})\"")]
    private static partial Regex ErrorCode();

    /// <summary>D82: the application starts a program only through Launcher, without a console window.</summary>
    [Fact]
    public void OnlyTheLauncher_StartsAProgram()
    {
        string app = Path.Combine(Repository(), "app");
        string[] offenders = Directory.EnumerateFiles(app, "*.cs", SearchOption.AllDirectories)
            .Where(f => !f.Contains($"{Path.DirectorySeparatorChar}obj{Path.DirectorySeparatorChar}") && !f.Contains($"{Path.DirectorySeparatorChar}bin{Path.DirectorySeparatorChar}"))
            .Where(f => !f.Contains($"{Path.DirectorySeparatorChar}CleanSlate.Tests{Path.DirectorySeparatorChar}") && !f.EndsWith("Launcher.cs", StringComparison.Ordinal))
            .Where(f => File.ReadAllText(f).Contains("Process.Start(", StringComparison.Ordinal))
            .ToArray();
        Assert.Empty(offenders);
        Assert.Contains("CreateNoWindow = true", File.ReadAllText(Path.Combine(app, "CleanSlate.App", "Services", "Launcher.cs")));
    }

    /// <summary>
    /// D91: running as administrator, the application never asks the shell to resolve a verb
    /// through the file associations. The only verb is <c>runas</c>, used by
    /// <c>App.RestartElevated</c> before CleanSlate has the administrator rights; the
    /// Properties window is opened by <c>SHObjectProperties</c>.
    /// </summary>
    [Fact]
    public void ElevatedApplication_NeverResolvesAShellVerb()
    {
        string app = Path.Combine(Repository(), "app");
        string sep = Path.DirectorySeparatorChar.ToString();
        var offenders = new List<string>();
        foreach (string file in Directory.EnumerateFiles(app, "*.cs", SearchOption.AllDirectories)
            .Where(f => !f.Contains($"{sep}obj{sep}") && !f.Contains($"{sep}bin{sep}") && !f.Contains($"{sep}CleanSlate.Tests{sep}")))
        {
            string text = File.ReadAllText(file);
            bool restart = file.EndsWith($"CleanSlate.App{sep}App.xaml.cs", StringComparison.Ordinal);
            foreach (Match verb in ShellVerb().Matches(text))
            {
                if (!(restart && verb.Groups[1].Value == "runas"))
                {
                    offenders.Add($"{file}: {verb.Value}");
                }
            }

            if (!restart && text.Contains("UseShellExecute = true", StringComparison.Ordinal))
            {
                offenders.Add($"{file}: UseShellExecute = true");
            }
        }

        Assert.Empty(offenders);
        Assert.Contains("SHObjectProperties(window", File.ReadAllText(Path.Combine(app, "CleanSlate.App", "Services", "EngineService.cs")));
    }

    [GeneratedRegex("Verb\\s*=\\s*\"([^\"]*)\"")]
    private static partial Regex ShellVerb();

    private static DiskMapNode Folder(string path, ulong size, bool? hasFolders, params DiskMapNode[] children) =>
        new(Path.GetFileName(path.TrimEnd('\\')) is { Length: > 0 } n ? n : path, path, size, 1, StorageCategory.UserFiles, children, false, true, hasFolders);

    private static (StorageMapViewModel Vm, IEngineService Engine, INavigationService Navigation, IStorageService Storage) Map(DiskMap map) =>
        Map(map, Substitute.For<IManagerService>(), Substitute.For<IDialogService>(), expert: false);

    private static (StorageMapViewModel Vm, IEngineService Engine, INavigationService Navigation, IStorageService Storage) Map(DiskMap map, IManagerService manager, IDialogService dialogs, bool expert)
    {
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 512 * Gb, 100 * Gb)]);
        storage.CachedMap("C:").Returns(_ => new CachedDiskMap(map, new DateTimeOffset(2026, 11, 4, 9, 0, 0, TimeSpan.FromHours(1))));
        storage.Journal().Returns([]);
        var engine = Substitute.For<IEngineService>();
        var navigation = Substitute.For<INavigationService>();
        storage.MeasureAsync(Arg.Any<StorageOperation>(), Arg.Any<Func<ActionResult, string>>(), Arg.Any<Func<Task<ActionResult>>>(), Arg.Any<Func<ActionResult, ulong>>(), Arg.Any<Func<ActionResult, Guid?>?>())
            .Returns(call => call.ArgAt<Func<Task<ActionResult>>>(2)());
        ISettingsService settings = OrphansViewModelTests.Settings();
        settings.ExpertMode.Returns(expert);
        var vm = new StorageMapViewModel(engine, storage, navigation, new OperationService(), manager, dialogs, settings);
        vm.Load();
        vm.Arrange(800, 500);
        return (vm, engine, navigation, storage);
    }

    // C: → Users → CapCut (not detailed) and Videos (terminal).
    private static DiskMap Drive() => new(
        Folder(@"C:\", 100 * Gb, true,
            Folder(@"C:\Users", 100 * Gb, true,
                Folder(@"C:\Users\CapCut", 60 * Gb, true),
                Folder(@"C:\Users\Videos", 40 * Gb, false))),
        512 * Gb, 100 * Gb, 0, 1, false);

    /// <summary>
    /// Point 4: a click on a level not computed yet shows a loading sign in its rectangle and
    /// waits for the engine; meanwhile the rest of the map still answers, and the level comes in
    /// only if the user is still where he clicked.
    /// </summary>
    [Fact]
    public async Task TheMap_StaysUsable_WhileALevelIsComputed()
    {
        var (vm, engine, navigation, _) = Map(Drive());
        var detail = new TaskCompletionSource<DiskMap>();
        engine.DiskMapAsync(Arg.Is<DiskMapRequest>(r => r.Root == @"C:\Users\CapCut"), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(detail.Task);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);

        Task pending = vm.OpenAsync(vm.Tiles.Single(t => t.Name == "CapCut"), TestContext.Current.CancellationToken);
        Assert.True(vm.Tiles.Single(t => t.Name == "CapCut").IsLoading);
        Assert.Equal(string.Empty, vm.ErrorText);

        // The user goes back to C: before the level arrives: he stays there.
        vm.GoTo(vm.Breadcrumb[0]);
        detail.SetResult(new DiskMap(Folder(@"C:\Users\CapCut", 60 * Gb, true, Folder(@"C:\Users\CapCut\Cache", 60 * Gb, false)), 512 * Gb, 100 * Gb, 0, 1, false));
        await pending;
        Assert.Equal(["C:"], vm.Breadcrumb.Select(b => b.Name));

        // Later, the computed level is used at once, without a new operation of the engine.
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "CapCut"), TestContext.Current.CancellationToken);
        Assert.Equal(["C:", "Users", "CapCut"], vm.Breadcrumb.Select(b => b.Name));
        Assert.Equal("Cache", Assert.Single(vm.Tiles).Name);
        await engine.Received(1).DiskMapAsync(Arg.Any<DiskMapRequest>(), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>());
    }

    private static FolderEntry Entry(string path, bool folder, ulong? size, Zone zone = Zone.Normal) =>
        new(Path.GetFileName(path), path, folder, size, new DateTimeOffset(2026, 11, 2, 10, 0, 0, TimeSpan.Zero), FileCategory.Videos, zone);

    /// <summary>
    /// D83: under the map, the folder it shows as a list, largest first, with the sizes the map
    /// already has; a folder of the list opens on both sides, and back on the page the map and
    /// the list are where they were.
    /// </summary>
    [Fact]
    public async Task TheExplorer_FollowsTheMap_AndOpensAFolderOnBothSides()
    {
        var manager = Substitute.For<IManagerService>();
        manager.ListFolderAsync(@"C:\Users", Arg.Any<IReadOnlyList<string>>(), Arg.Any<CancellationToken>())
            .Returns([Entry(@"C:\Users\notes.txt", false, Gb), Entry(@"C:\Users\Videos", true, null), Entry(@"C:\Users\CapCut", true, null)]);
        manager.ListFolderAsync(@"C:\Users\Videos", Arg.Any<IReadOnlyList<string>>(), Arg.Any<CancellationToken>())
            .Returns([Entry(@"C:\Users\Videos\film.mp4", false, 40 * Gb)]);
        var (vm, _, navigation, _) = Map(Drive(), manager, Substitute.For<IDialogService>(), expert: false);

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        await vm.ListingTask;
        Assert.Equal(@"C:\Users", vm.FolderText);
        Assert.Equal(["CapCut", "Videos", "notes.txt"], vm.Entries.Select(e => e.Name));
        Assert.Equal(Formats.Size(60 * Gb), vm.Entries[0].SizeText); // the size of the map
        await manager.Received(1).ListFolderAsync(@"C:\Users", Arg.Is<IReadOnlyList<string>>(k => k.Count == 2), Arg.Any<CancellationToken>());

        await vm.OpenEntryAsync(vm.Entries.Single(e => e.Name == "Videos"), TestContext.Current.CancellationToken);
        await vm.ListingTask;
        Assert.Equal(["C:", "Users", "Videos"], vm.Breadcrumb.Select(b => b.Name));
        Assert.Equal("film.mp4", Assert.Single(vm.Entries).Name);
        Assert.Equal("Vidéos · .mp4", vm.Entries[0].TypeText);

        vm.Load(); // the page is shown again
        Assert.Equal(["C:", "Users", "Videos"], vm.Breadcrumb.Select(b => b.Name));
        Assert.Empty(navigation.ReceivedCalls());
    }

    /// <summary>D83: the checked files are deleted after the confirmation; a sensitive one needs the expert mode first.</summary>
    [Fact]
    public async Task TheExplorer_DeletesTheCheckedFiles_AfterTheConfirmation()
    {
        var manager = Substitute.For<IManagerService>();
        manager.ListFolderAsync(@"C:\Users\Videos", Arg.Any<IReadOnlyList<string>>(), Arg.Any<CancellationToken>())
            .Returns([Entry(@"C:\Users\Videos\film.mp4", false, 40 * Gb), Entry(@"C:\Users\Videos\system.dll", false, Gb, Zone.Sensitive)]);
        manager.DeleteAsync(Arg.Any<IReadOnlyList<string>>(), Arg.Any<DeleteMode>(), Arg.Any<ManualAction>(), Arg.Any<CancellationToken>()).Returns(new ActionResult([], 0, 0, false));
        var dialogs = Substitute.For<IDialogService>();
        dialogs.ConfirmManagerAsync(Arg.Any<ManagerSummary>()).Returns(new ManagerDecision(true, DeleteMode.Immediate));
        var (vm, _, _, _) = Map(Drive(), manager, dialogs, expert: false);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Videos"), TestContext.Current.CancellationToken);
        await vm.ListingTask;

        vm.CheckAllCommand.Execute(null);
        await vm.DeleteCheckedCommand.ExecuteAsync(null);
        await dialogs.DidNotReceive().ConfirmManagerAsync(Arg.Any<ManagerSummary>());
        Assert.Contains("mode expert", vm.ExplorerResult);

        vm.UncheckAllCommand.Execute(null);
        await vm.OpenEntryAsync(vm.Entries.Single(e => e.Name == "film.mp4"), TestContext.Current.CancellationToken); // a click checks a file
        await vm.DeleteCheckedCommand.ExecuteAsync(null);
        await dialogs.Received(1).ConfirmManagerAsync(Arg.Is<ManagerSummary>(m => m.ItemCount == 1 && m.OfferModes));
        await manager.Received(1).DeleteAsync(Arg.Is<IReadOnlyList<string>>(p => p.Single() == @"C:\Users\Videos\film.mp4"), DeleteMode.Immediate, Arg.Any<ManualAction>(), Arg.Any<CancellationToken>());
        await manager.Received(2).ListFolderAsync(@"C:\Users\Videos", Arg.Any<IReadOnlyList<string>>(), Arg.Any<CancellationToken>()); // read again
    }

    /// <summary>
    /// D84: a level of the map is drawn once. The rectangles were cleared then added one by
    /// one, and the page drew the whole map at each addition (40 rectangles, about 800 buttons).
    /// </summary>
    [Fact]
    public async Task ALevelOfTheMap_IsDrawnOnce()
    {
        var (vm, _, _, _) = Map(Drive());
        int notifications = 0;
        vm.Tiles.CollectionChanged += (_, _) => notifications++;

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        Assert.Equal(1, notifications);
        Assert.Equal(2, vm.Tiles.Count);

        vm.Arrange(1000, 600); // a resize of the window
        Assert.Equal(2, notifications);
    }

    /// <summary>Point 3: during an analysis the button says so, and the indicator shows it on every page.</summary>
    [Fact]
    public async Task AnAnalysis_IsShownByTheButtonAndTheIndicator_UntilItEnds()
    {
        var manager = Substitute.For<IManagerService>();
        var analysis = new TaskCompletionSource<StorageAnalysis>();
        manager.AnalyzeAsync(Arg.Any<string>(), Arg.Any<IProgress<ManagerProgress>?>(), Arg.Any<CancellationToken>()).Returns(analysis.Task);
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 100 * Gb, 6 * Gb)]);
        var operations = new OperationService();
        var vm = new ManagerViewModel(manager, storage, Substitute.For<IDialogService>(), OrphansViewModelTests.Settings(), Substitute.For<IEngineService>(), Substitute.For<INavigationService>(), operations);

        Task running = vm.AnalyzeCommand.ExecuteAsync(null);
        Assert.True(vm.IsAnalyzing);
        Assert.True(operations.IsActive);
        Assert.Equal("Manager", operations.PageTag);
        Assert.StartsWith("Analyse de", operations.Text);

        analysis.SetResult(new StorageAnalysis(@"C:\", [], [], [], [], [], 0, 0, 0, false));
        await running;
        Assert.False(vm.IsAnalyzing);
        Assert.False(operations.IsActive);
    }
}
