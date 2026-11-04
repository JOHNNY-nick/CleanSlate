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

    private static DiskMapNode Folder(string path, ulong size, bool? hasFolders, params DiskMapNode[] children) =>
        new(Path.GetFileName(path.TrimEnd('\\')) is { Length: > 0 } n ? n : path, path, size, 1, StorageCategory.UserFiles, children, false, true, hasFolders);

    private static (StorageMapViewModel Vm, IEngineService Engine, INavigationService Navigation, IStorageService Storage) Map(DiskMap map)
    {
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 512 * Gb, 100 * Gb)]);
        storage.CachedMap("C:").Returns(_ => new CachedDiskMap(map, new DateTimeOffset(2026, 11, 4, 9, 0, 0, TimeSpan.FromHours(1))));
        storage.Journal().Returns([]);
        var engine = Substitute.For<IEngineService>();
        var navigation = Substitute.For<INavigationService>();
        var vm = new StorageMapViewModel(engine, storage, navigation, new OperationService());
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

        // A terminal folder: a single click does nothing, a double-click opens its files.
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Videos"), TestContext.Current.CancellationToken);
        navigation.DidNotReceive().OpenManager(Arg.Any<string>());
        vm.DoubleClick(vm.Tiles.Single(t => t.Name == "Videos"));
        navigation.Received(1).OpenManager(@"C:\Users\Videos");

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

    /// <summary>Point 5: back from the Gestionnaire, the map is at the same place; right-click « Voir les fichiers » on any folder.</summary>
    [Fact]
    public async Task BackToTheMap_ItIsAtTheSamePlace_AndAnyFolderShowsItsFiles()
    {
        var (vm, _, navigation, _) = Map(Drive());
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);

        vm.ShowFiles(vm.Tiles.Single(t => t.Name == "CapCut"));
        navigation.Received(1).OpenManager(@"C:\Users\CapCut");

        vm.Load(); // the page is shown again
        Assert.Equal(["C:", "Users"], vm.Breadcrumb.Select(b => b.Name));
        Assert.Contains(vm.Tiles, t => t.Name == "CapCut");
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
