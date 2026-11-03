using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Quarantine from the Orphans page and the History page (EF-16 to EF-20, ENF-02).</summary>
public sealed class QuarantineViewModelTests
{
    private static readonly Leftover High = new("h1", LeftoverKind.Folder, @"C:\A\x", 1_000, null, Confidence.High, [], "x");
    private static readonly Leftover Key = new("k1", LeftoverKind.RegistryKey, @"HKCU\Software\X", 10, null, Confidence.High, [], "X");

    private static ScanReport Report() => new([new LeftoverGroup("x", null, [High, Key], 1_010)], [], 1, false);

    private static QuarantinedItem Moved(Leftover l, QuarantineStatus status) => new(
        l.Id, l.Kind, l.Location, @"C:\Q\" + l.Id, l.SizeBytes, null, null, DateTimeOffset.Now, null, status, null);

    private static Session SessionOf(bool simulated, params QuarantinedItem[] items) => new(
        Guid.NewGuid(), DateTimeOffset.Now, Origin.Orphans, items, 1_000, [], SessionStatus.Completed, simulated);

    private static async Task<(OrphansViewModel Vm, IEngineService Engine)> ScannedAsync(
        IDialogService dialogs, ISettingsService settings)
    {
        var engine = Substitute.For<IEngineService>();
        engine.ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(Report());
        var vm = new OrphansViewModel(engine, dialogs, settings, Substitute.For<INavigationService>());
        await vm.ScanCommand.ExecuteAsync(null);
        return (vm, engine);
    }

    [Fact]
    public async Task Quarantine_AlwaysAsksForConfirmation_AndStopsWhenCancelled()
    {
        var dialogs = OrphansViewModelTests.Dialogs(confirm: false);
        var (vm, engine) = await ScannedAsync(dialogs, OrphansViewModelTests.Settings());

        Assert.True(vm.QuarantineCommand.CanExecute(null));
        await vm.QuarantineCommand.ExecuteAsync(null);

        await dialogs.Received(1).ConfirmCleanAsync(Arg.Is<CleanSummary>(s => s.ItemCount == 2 && !s.DeleteImmediately && s.QuarantineDays == 30));
        await engine.DidNotReceive().CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Quarantine_DeleteImmediately_PurgesThatSessionOnly()
    {
        // « Supprimer définitivement, sans quarantaine » (D66): the items go through the
        // quarantine (manifest first), then that one session is purged at once.
        var dialogs = OrphansViewModelTests.Dialogs();
        var (vm, engine) = await ScannedAsync(dialogs, OrphansViewModelTests.Settings(deleteImmediately: true));
        Session session = SessionOf(false, Moved(High, QuarantineStatus.Quarantined), Moved(Key, QuarantineStatus.Quarantined));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>()).Returns(session);

        await vm.QuarantineCommand.ExecuteAsync(null);

        await dialogs.Received(1).ConfirmCleanAsync(Arg.Is<CleanSummary>(s => s.DeleteImmediately));
        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => !p.DryRun), Arg.Any<CancellationToken>());
        await engine.Received(1).PurgeAsync(Arg.Is<PurgeRequest>(p => p.SessionId == session.Id && !p.DryRun), Arg.Any<CancellationToken>());
        Assert.True(vm.DeletedForGood);
        Assert.Equal(2, vm.CleanedCount);
    }

    [Fact]
    public async Task Quarantine_Real_RemovesMovedItemsAndReportsOutcome()
    {
        var (vm, engine) = await ScannedAsync(OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings());
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>())
            .Returns(SessionOf(false, Moved(High, QuarantineStatus.Quarantined), Moved(Key, QuarantineStatus.Failed)));

        await vm.QuarantineCommand.ExecuteAsync(null);

        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => !p.DryRun), Arg.Any<CancellationToken>());
        await engine.DidNotReceive().PurgeAsync(Arg.Any<PurgeRequest>(), Arg.Any<CancellationToken>());
        Assert.False(vm.DeletedForGood);
        Assert.True(vm.ShowCleanResult);
        Assert.Equal(1, vm.CleanedCount);
        Assert.Equal(1, vm.CleanFailureCount);
        LeftoverItemViewModel left = Assert.Single(vm.Groups[0].Items, i => i.IsVisible);
        Assert.Equal("k1", left.Leftover.Id);
    }

    [Fact]
    public async Task MovedItems_AreHidden_WithoutChangingTheGroupsItems()
    {
        // Crash on the owner's PC, 31 Oct. 2026 (D70): removing a moved item from a group's
        // list, shown by an ItemsRepeater inside an Expander, threw COMException 0x80004005
        // in WinUI. The nested lists are never changed: moved items are hidden instead.
        var (vm, engine) = await ScannedAsync(OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings());
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>())
            .Returns(SessionOf(false, Moved(High, QuarantineStatus.Quarantined), Moved(Key, QuarantineStatus.Failed)));
        LeftoverGroupViewModel group = vm.Groups[0];
        int changes = 0;
        group.Items.CollectionChanged += (_, _) => changes++;

        await vm.QuarantineCommand.ExecuteAsync(null);

        Assert.Equal(0, changes);
        LeftoverItemViewModel moved = group.Items.Single(i => i.Leftover.Id == "h1");
        Assert.True(moved.IsDone);
        Assert.False(moved.IsVisible);
        Assert.False(moved.IsChecked);
        Assert.True(group.Items.Single(i => i.Leftover.Id == "k1").IsVisible);
        Assert.Equal(1, vm.SelectedCount); // the failed item stays checked, ready to retry

        // A filter never brings a moved item back.
        vm.FilterIndex = 1;
        vm.FilterIndex = 0;
        Assert.False(moved.IsVisible);
    }

    [Fact]
    public async Task NothingMoved_ShowsNoSuccessLine()
    {
        // Installer run of 31 Oct. 2026 (D67): « 0 élément mis en quarantaine… restaurables »
        // was shown next to « 1 élément laissé en place ».
        var (vm, engine) = await ScannedAsync(OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(deleteImmediately: true));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>())
            .Returns(SessionOf(false, Moved(High, QuarantineStatus.Failed)));

        await vm.QuarantineCommand.ExecuteAsync(null);

        Assert.True(vm.ShowCleanResult);
        Assert.False(vm.HasCleaned);
        Assert.Equal(1, vm.CleanFailureCount);
        await engine.DidNotReceive().PurgeAsync(Arg.Any<PurgeRequest>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Quarantine_AboveCeiling_AsksForTheWord()
    {
        var dialogs = OrphansViewModelTests.Dialogs(confirm: true, word: "QUARANTAINE");
        var (vm, engine) = await ScannedAsync(dialogs, OrphansViewModelTests.Settings());
        engine.CleanAsync(Arg.Is<CleanPlan>(p => p.Confirmation == null), Arg.Any<CancellationToken>())
            .Returns<Task<Session>>(_ => throw new EngineException("CONFIRMATION_REQUIRED", "too big"));
        engine.CleanAsync(Arg.Is<CleanPlan>(p => p.Confirmation == "QUARANTAINE"), Arg.Any<CancellationToken>())
            .Returns(SessionOf(false, Moved(High, QuarantineStatus.Quarantined)));

        await vm.QuarantineCommand.ExecuteAsync(null);

        await dialogs.Received(1).ConfirmCleanAsync(Arg.Is<CleanSummary>(s => s.RequiresWord));
        Assert.Equal(1, vm.CleanedCount);
    }

    [Fact]
    public async Task History_LoadsRestoresCheckedItemsAndPurges()
    {
        var engine = Substitute.For<IEngineService>();
        Session session = SessionOf(false, Moved(High, QuarantineStatus.Quarantined), Moved(Key, QuarantineStatus.Quarantined));
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns([session]);
        engine.RestoreAsync(Arg.Any<RestoreRequest>(), Arg.Any<CancellationToken>())
            .Returns(session with { Items = [Moved(High, QuarantineStatus.Restored), Moved(Key, QuarantineStatus.Quarantined)] });
        engine.PurgeAsync(Arg.Any<PurgeRequest>(), Arg.Any<CancellationToken>()).Returns(new PurgeResult(2048, false));
        var vm = new HistoryViewModel(engine, OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(), Substitute.For<INavigationService>());

        await vm.LoadCommand.ExecuteAsync(null);
        Assert.False(vm.IsEmpty);
        SessionViewModel row = Assert.Single(vm.Sessions);
        Assert.True(row.CanRestore);
        row.Items[0].IsChecked = true;

        await vm.RestoreCommand.ExecuteAsync(row);
        await engine.Received(1).RestoreAsync(
            Arg.Is<RestoreRequest>(r => !r.DryRun && r.ItemIds!.Count == 1 && r.ItemIds[0] == "h1"),
            Arg.Any<CancellationToken>());
        Assert.Equal(1, vm.RestoredCount);

        await vm.PurgeCommand.ExecuteAsync(null);
        await engine.Received(1).PurgeAsync(Arg.Is<PurgeRequest>(p => p.OlderThanDays == 30 && !p.DryRun), Arg.Any<CancellationToken>());
        Assert.Equal("2 Ko", vm.PurgedText);
    }

    [Fact]
    public async Task History_RestoreAll_IsReal()
    {
        // No simulation mode in the application any more (D66): restoring always acts.
        var engine = Substitute.For<IEngineService>();
        Session session = SessionOf(false, Moved(High, QuarantineStatus.Quarantined));
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns([session]);
        engine.RestoreAsync(Arg.Any<RestoreRequest>(), Arg.Any<CancellationToken>()).Returns(session with { Items = [Moved(High, QuarantineStatus.Restored)] });
        var vm = new HistoryViewModel(engine, OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(), Substitute.For<INavigationService>());
        await vm.LoadCommand.ExecuteAsync(null);

        await vm.RestoreCommand.ExecuteAsync(vm.Sessions[0]);

        await engine.Received(1).RestoreAsync(Arg.Is<RestoreRequest>(r => !r.DryRun && r.ItemIds == null), Arg.Any<CancellationToken>());
        Assert.False(vm.LastWasSimulated);
    }

    [Fact]
    public async Task EmptyHistory_OffersToAnalyzeLeftovers()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns(Array.Empty<Session>());
        var navigation = Substitute.For<INavigationService>();
        var vm = new HistoryViewModel(engine, OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(), navigation);
        await vm.LoadCommand.ExecuteAsync(null);

        Assert.True(vm.IsEmpty);
        vm.AnalyzeLeftoversCommand.Execute(null);

        navigation.Received(1).OpenOrphans(true);
    }

    [Fact]
    public void Settings_ExclusionsAndTheme()
    {
        var settings = OrphansViewModelTests.Settings();
        var stored = new List<string>();
        settings.Exclusions.Returns(_ => stored.ToList());
        settings.When(s => s.AddExclusion(Arg.Any<string>())).Do(c => stored.Add(c.Arg<string>().Trim()));
        var vm = new SettingsViewModel(settings);

        vm.NewExclusion = @"relative\path";
        Assert.False(vm.AddExclusionCommand.CanExecute(null));
        vm.NewExclusion = @"C:\Users\Ana\AppData\Roaming\KeepMe";
        Assert.True(vm.AddExclusionCommand.CanExecute(null));
        vm.AddExclusionCommand.Execute(null);
        Assert.Single(vm.Exclusions);
        Assert.Equal(string.Empty, vm.NewExclusion);
        Assert.True(SettingsViewModel.IsValidExclusion(@"HKCU\Software\KeepMe"));

        vm.RemoveExclusionCommand.Execute(@"C:\Users\Ana\AppData\Roaming\KeepMe");
        settings.Received(1).RemoveExclusion(@"C:\Users\Ana\AppData\Roaming\KeepMe");
        Assert.Empty(vm.Exclusions);

        vm.ThemeIndex = 2;
        settings.Received().Theme = AppTheme.Dark;
    }

    [Fact]
    public void Settings_ClampDaysAndWriteThrough()
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings);

        vm.DeleteImmediately = true;
        vm.QuarantineDays = 500;

        settings.Received().DeleteImmediately = true;
        settings.Received().QuarantineDays = 365;
        Assert.Equal(365, vm.QuarantineDays);
    }
}
