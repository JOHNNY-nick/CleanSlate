using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Storage map (D73): treemap layout, update after a cleanup, and its ViewModel.</summary>
public sealed class StorageMapTests
{
    private const ulong Gb = 1024UL * 1024 * 1024;

    private static DiskMapNode Node(string path, ulong size, StorageCategory category, params DiskMapNode[] children) =>
        new(Path.GetFileName(path.TrimEnd('\\')) is { Length: > 0 } n ? n : path, path, size, 1, category, children, false, true);

    private static DiskMap Machine() => new(
        Node(@"C:\", 412 * Gb, StorageCategory.Other,
            Node(@"C:\Windows", 40 * Gb, StorageCategory.System),
            Node(@"C:\Program Files", 120 * Gb, StorageCategory.Applications,
                Node(@"C:\Program Files\VideoLAN", 1 * Gb, StorageCategory.Applications),
                Node(@"C:\Program Files\Steam", 119 * Gb, StorageCategory.Applications)),
            Node(@"C:\Users", 250 * Gb, StorageCategory.UserFiles,
                Node(@"C:\Users\Ana", 250 * Gb, StorageCategory.UserFiles,
                    Node(@"C:\Users\Ana\AppData", 50 * Gb, StorageCategory.Other,
                        Node(@"C:\Users\Ana\AppData\Roaming\OldTool", 2 * Gb, StorageCategory.Leftovers),
                        Node(@"C:\Users\Ana\AppData\Local\CleanSlate", 1 * Gb, StorageCategory.Quarantine)),
                    Node(@"C:\Users\Ana\Videos", 200 * Gb, StorageCategory.UserFiles))),
            new DiskMapNode("Autres petits éléments", "", 2 * Gb, 900, StorageCategory.Other, [], true, false)),
        TotalBytes: 512 * Gb, FreeBytes: 100 * Gb, Denied: 3, DurationMs: 9_000, Cancelled: false);

    [Fact]
    public void Treemap_AreasAreProportional_AndStayInside()
    {
        ulong[] sizes = [60, 30, 10, 0, 5];
        var rects = Treemap.Layout(sizes, 400, 200);
        Assert.Equal(4, rects.Count); // the empty item gets nothing
        double total = rects.Sum(r => r.Width * r.Height);
        Assert.Equal(400 * 200, total, 3);
        foreach (TreemapRect r in rects)
        {
            Assert.InRange(r.X, -0.001, 400.001);
            Assert.InRange(r.Y, -0.001, 200.001);
            Assert.True(r.X + r.Width <= 400.001 && r.Y + r.Height <= 200.001);
            Assert.Equal(sizes[r.Index] / 105.0, r.Width * r.Height / total, 3);
        }

        // No overlap.
        for (int i = 0; i < rects.Count; i++)
        {
            for (int j = i + 1; j < rects.Count; j++)
            {
                TreemapRect a = rects[i], b = rects[j];
                bool apart = a.X + a.Width <= b.X + 0.001 || b.X + b.Width <= a.X + 0.001 || a.Y + a.Height <= b.Y + 0.001 || b.Y + b.Height <= a.Y + 0.001;
                Assert.True(apart, $"{a} / {b}");
            }
        }
    }

    [Fact]
    public void Treemap_KeepsRectanglesReasonablySquare()
    {
        var rects = Treemap.Layout(Enumerable.Repeat(10UL, 16).ToList(), 400, 400);
        Assert.All(rects, r => Assert.InRange(Math.Max(r.Width / r.Height, r.Height / r.Width), 1, 2.5));
    }

    [Fact]
    public void AfterACleanup_TheMovedFolderLeaves_AndTheQuarantineGrows()
    {
        DiskMap map = DiskMapUpdate.Moved(Machine(), [(@"C:\Users\Ana\AppData\Roaming\OldTool", 2 * Gb)], @"C:\Users\Ana\AppData\Local\CleanSlate");
        DiskMapNode appData = map.Root.Children.Single(c => c.Name == "Users").Children[0].Children.Single(c => c.Name == "AppData");
        Assert.DoesNotContain(appData.Children, c => c.Name == "OldTool");
        Assert.Equal(3 * Gb, appData.Children.Single(c => c.Category == StorageCategory.Quarantine).SizeBytes);
        Assert.Equal(50 * Gb, appData.SizeBytes); // moved on the same disk: same used space
        Assert.Equal(412 * Gb, map.Root.SizeBytes);

        DiskMap purged = DiskMapUpdate.Purged(map, 3 * Gb, @"C:\Users\Ana\AppData\Local\CleanSlate");
        Assert.Equal(409 * Gb, purged.Root.SizeBytes);
        Assert.Equal(103 * Gb, purged.FreeBytes);
    }

    private static (StorageMapViewModel Vm, IStorageService Storage, INavigationService Navigation) Create(DiskMap? cached = null)
    {
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 512 * Gb, 100 * Gb)]);
        storage.CachedMap("C:").Returns(cached is null ? null : new CachedDiskMap(cached, new DateTimeOffset(2026, 11, 3, 18, 30, 0, TimeSpan.FromHours(1))));
        storage.Journal().Returns([]);
        var engine = Substitute.For<IEngineService>();
        engine.DiskMapAsync(Arg.Any<DiskMapRequest>(), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>()).Returns(Machine());
        var navigation = Substitute.For<INavigationService>();
        return (new StorageMapViewModel(engine, storage, navigation), storage, navigation);
    }

    [Fact]
    public void TheDriveBar_SaysUsedAndFree_InPlainWords()
    {
        var (vm, _, _) = Create(Machine());
        vm.Load();
        DriveBar bar = Assert.Single(vm.Drives);
        Assert.Equal("412 Go utilisés sur 512 Go — 100 Go libres", bar.Text);
        Assert.Equal(80, bar.UsedPercent);
    }

    [Fact]
    public async Task Refresh_ComputesTheMap_CachesIt_AndShowsTheTime()
    {
        var (vm, storage, _) = Create();
        vm.Load();
        Assert.False(vm.HasMap);

        await vm.RefreshCommand.ExecuteAsync(null);

        Assert.True(vm.HasMap);
        storage.Received(1).SaveMap("C:", Arg.Any<DiskMap>());
        Assert.False(string.IsNullOrEmpty(vm.LastComputedText));
    }

    [Fact]
    public void Tiles_HaveNameSizeShareAndCategory_AndTheLegendNamesEachCategory()
    {
        var (vm, _, _) = Create(Machine());
        vm.Load();
        vm.Arrange(800, 500);

        Assert.Equal(4, vm.Tiles.Count);
        StorageTile users = vm.Tiles.Single(t => t.Name == "Users");
        Assert.Equal("250 Go", users.SizeText);
        Assert.Contains("49 %", users.ToolTip); // 250 of 512 Go
        Assert.Equal(StorageCategory.UserFiles, users.Category);
        Assert.True(users.ShowLabel);
        // Categories are told by their names, not only by color.
        Assert.Contains(vm.Legend, l => l.Category == StorageCategory.Leftovers);
        Assert.All(vm.Legend, l => Assert.False(string.IsNullOrEmpty(l.SizeText)));
    }

    [Fact]
    public async Task Clicking_GoesDownToThreeLevels_WithABreadcrumbToGoBack()
    {
        var (vm, _, _) = Create(Machine());
        vm.Load();
        vm.Arrange(800, 500);

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Ana"), TestContext.Current.CancellationToken);
        Assert.Equal(["C:", "Users", "Ana"], vm.Breadcrumb.Select(b => b.Name));
        Assert.Contains(vm.Tiles, t => t.Name == "AppData");

        vm.GoTo(vm.Breadcrumb[0]);
        Assert.Equal(["C:"], vm.Breadcrumb.Select(b => b.Name));
    }

    [Fact]
    public async Task Clicking_LeftoversOrAnApplication_OpensItsPage()
    {
        var (vm, _, navigation) = Create(Machine());
        vm.Load();
        vm.Arrange(800, 500);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Users"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Ana"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "AppData"), TestContext.Current.CancellationToken);

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "OldTool"), TestContext.Current.CancellationToken);
        navigation.Received(1).OpenOrphans(false);

        vm.GoTo(vm.Breadcrumb[0]);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Program Files"), TestContext.Current.CancellationToken);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Steam"), TestContext.Current.CancellationToken);
        navigation.Received(1).OpenApps();
    }

    [Fact]
    public void TheLastCleanup_IsOneShortLine()
    {
        var (vm, storage, _) = Create(Machine());
        storage.Journal().Returns(
        [
            new StorageEntry(DateTimeOffset.Now.AddMinutes(-9), StorageOperation.Purge, "C:", 100 * Gb, 101 * Gb, Gb, "30 jours"),
            new StorageEntry(DateTimeOffset.Now, StorageOperation.DeleteForGood, "C:", 101 * Gb, (ulong)(102.2 * Gb), (ulong)(1.2 * Gb), "session"),
        ]);
        vm.Load();
        Assert.Equal("+ 1,2 Go libérés par le dernier nettoyage", vm.LastCleanupText);
    }

    [Fact]
    public async Task Home_ShowsACompactPreview_OfTheSystemDrive()
    {
        var (_, storage, _) = Create(Machine());
        var navigation = Substitute.For<INavigationService>();
        var home = new HomeViewModel(Substitute.For<IEngineService>(), navigation: navigation, storage: storage);

        await home.LoadCommand.ExecuteAsync(null);

        Assert.Equal("412 Go utilisés sur 512 Go — 100 Go libres", home.StorageDrive?.Text);
        // The three largest categories, by name and size.
        Assert.Equal([StorageCategory.UserFiles, StorageCategory.Applications, StorageCategory.System], home.StorageSummary.Select(i => i.Category));
        home.OpenStorageCommand.Execute(null);
        navigation.Received(1).OpenStorage();
    }

    [Fact]
    public async Task History_ShowsTheFreeSpaceDetail_InTheSessionConcerned()
    {
        var id = Guid.NewGuid();
        var storage = Substitute.For<IStorageService>();
        storage.Journal().Returns(
        [
            new StorageEntry(DateTimeOffset.Now, StorageOperation.Quarantine, "C:", 100 * Gb, 100 * Gb, Gb, "1 élément", id),
            new StorageEntry(DateTimeOffset.Now, StorageOperation.Purge, "C:", 100 * Gb, 102 * Gb, 2 * Gb, "30 jours"),
        ]);
        var engine = Substitute.For<IEngineService>();
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns(
            [new Session(id, DateTimeOffset.Now, Origin.Orphans, [], Gb, [], SessionStatus.Completed, false)]);
        var history = new HistoryViewModel(engine, Substitute.For<IDialogService>(), Substitute.For<ISettingsService>(), Substitute.For<INavigationService>(), storage);

        await history.LoadCommand.ExecuteAsync(null);

        SessionStorageRow row = Assert.Single(Assert.Single(history.Sessions).StorageRows);
        Assert.Equal(StorageOperation.Quarantine, row.Operation);
        Assert.Equal("100 Go", row.BeforeText);
        Assert.Equal("0 octets", row.DifferenceText);
    }

    /// <summary>D81: below the levels of the map, a folder gets a map of its own (no depth limit).</summary>
    [Fact]
    public async Task AFolderNotDetailedYet_GetsAMapOfItsOwn()
    {
        DiskMapNode downloads = Node(@"C:\Users\Ana\Downloads", 30 * Gb, StorageCategory.UserFiles) with { HasFolders = true };
        DiskMapNode setups = Node(@"C:\Users\Ana\Downloads\Setups", 20 * Gb, StorageCategory.UserFiles) with { HasFolders = false };
        DiskMapNode detail = Node(@"C:\Users\Ana\Downloads", 30 * Gb, StorageCategory.UserFiles, setups) with { HasFolders = true };
        var map = new DiskMap(Node(@"C:\", 30 * Gb, StorageCategory.Other, downloads) with { HasFolders = true }, 512 * Gb, 100 * Gb, 0, 1, false);
        var (vm, _, navigation) = Create(map);
        var engine = Substitute.For<IEngineService>();
        engine.DiskMapAsync(Arg.Is<DiskMapRequest>(r => r.Root == downloads.Path), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new DiskMap(detail, 512 * Gb, 100 * Gb, 0, 1, false));
        vm = new StorageMapViewModel(engine, StorageOf(map), navigation);
        vm.Load();
        vm.Arrange(800, 500);

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Downloads"), TestContext.Current.CancellationToken);

        Assert.Equal(["C:", "Downloads"], vm.Breadcrumb.Select(b => b.Name));
        StorageTile tile = Assert.Single(vm.Tiles);
        Assert.Equal("Setups", tile.Name);
        Assert.Contains("Gestionnaire", tile.ToolTip);

        // Back up and down again: the detail is kept for the session, computed once.
        vm.GoTo(vm.Breadcrumb[0]);
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Downloads"), TestContext.Current.CancellationToken);
        await engine.Received(1).DiskMapAsync(Arg.Any<DiskMapRequest>(), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>());

        // A terminal folder opens the Gestionnaire with its files, without a new map.
        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "Setups"), TestContext.Current.CancellationToken);
        navigation.Received(1).OpenManager(setups.Path);
        await engine.Received(1).DiskMapAsync(Arg.Any<DiskMapRequest>(), Arg.Any<IProgress<DiskMapProgress>?>(), Arg.Any<CancellationToken>());
    }

    /// <summary>D81: a file opens the Gestionnaire on its folder; a small rectangle still shows its name.</summary>
    [Fact]
    public async Task AFile_OpensTheGestionnaireOnItsFolder_AndSmallRectanglesShowTheirName()
    {
        var file = new DiskMapNode("film.mp4", @"C:\Users\Ana\Videos\film.mp4", 10 * Gb, 1, StorageCategory.UserFiles, [], false, false);
        var tiny = Node(@"C:\Users\Ana\Videos\Clips", 1 * Gb, StorageCategory.UserFiles) with { HasFolders = false };
        var map = new DiskMap(Node(@"C:\", 11 * Gb, StorageCategory.Other, file, tiny) with { HasFolders = true }, 512 * Gb, 100 * Gb, 0, 1, false);
        var (vm, _, navigation) = Create(map);
        vm.Load();
        vm.Arrange(800, 500);

        StorageTile small = vm.Tiles.Single(t => t.Name == "Clips");
        Assert.True(small.ShowLabel, $"{small.Width} x {small.Height}: the name is shown");
        Assert.False(small.ShowSize);

        await vm.OpenAsync(vm.Tiles.Single(t => t.Name == "film.mp4"), TestContext.Current.CancellationToken);
        navigation.Received(1).OpenManager(@"C:\Users\Ana\Videos");
    }

    private static IStorageService StorageOf(DiskMap map)
    {
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 512 * Gb, 100 * Gb)]);
        storage.CachedMap("C:").Returns(new CachedDiskMap(map, DateTimeOffset.Now));
        storage.Journal().Returns([]);
        return storage;
    }
}
