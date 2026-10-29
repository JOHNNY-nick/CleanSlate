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
        var vm = new OrphansViewModel(engine, dialogs, settings);
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

        await dialogs.Received(1).ConfirmCleanAsync(Arg.Is<CleanSummary>(s => s.ItemCount == 2 && s.Simulation && s.QuarantineDays == 30));
        await engine.DidNotReceive().CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Quarantine_InSimulation_SendsDryRunAndKeepsItems()
    {
        var (vm, engine) = await ScannedAsync(OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(simulation: true));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>()).Returns(SessionOf(true));

        await vm.QuarantineCommand.ExecuteAsync(null);

        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => p.DryRun && p.ItemIds.Count == 2), Arg.Any<CancellationToken>());
        Assert.True(vm.ShowSimulation);
        Assert.Equal(2, vm.Groups[0].Items.Count);
    }

    [Fact]
    public async Task Quarantine_Real_RemovesMovedItemsAndReportsOutcome()
    {
        var (vm, engine) = await ScannedAsync(OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(simulation: false));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>())
            .Returns(SessionOf(false, Moved(High, QuarantineStatus.Quarantined), Moved(Key, QuarantineStatus.Failed)));

        await vm.QuarantineCommand.ExecuteAsync(null);

        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => !p.DryRun), Arg.Any<CancellationToken>());
        Assert.True(vm.ShowCleanResult);
        Assert.Equal(1, vm.CleanedCount);
        Assert.Equal(1, vm.CleanFailureCount);
        Assert.Single(vm.Groups[0].Items);
        Assert.Equal("k1", vm.Groups[0].Items[0].Leftover.Id);
    }

    [Fact]
    public async Task Quarantine_AboveCeiling_AsksForTheWord()
    {
        var dialogs = OrphansViewModelTests.Dialogs(confirm: true, word: "QUARANTAINE");
        var (vm, engine) = await ScannedAsync(dialogs, OrphansViewModelTests.Settings(simulation: false));
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
        var vm = new HistoryViewModel(engine, OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(simulation: false));

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
    public async Task History_RestoreAll_InSimulation()
    {
        var engine = Substitute.For<IEngineService>();
        Session session = SessionOf(false, Moved(High, QuarantineStatus.Quarantined));
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns([session]);
        engine.RestoreAsync(Arg.Any<RestoreRequest>(), Arg.Any<CancellationToken>()).Returns(session with { Simulated = true });
        var vm = new HistoryViewModel(engine, OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(simulation: true));
        await vm.LoadCommand.ExecuteAsync(null);

        await vm.RestoreCommand.ExecuteAsync(vm.Sessions[0]);

        await engine.Received(1).RestoreAsync(Arg.Is<RestoreRequest>(r => r.DryRun && r.ItemIds == null), Arg.Any<CancellationToken>());
        Assert.True(vm.LastWasSimulated);
    }

    [Fact]
    public void Settings_ClampDaysAndWriteThrough()
    {
        var settings = OrphansViewModelTests.Settings();
        var vm = new SettingsViewModel(settings);

        vm.SimulationMode = false;
        vm.QuarantineDays = 500;

        settings.Received().SimulationMode = false;
        settings.Received().QuarantineDays = 365;
        Assert.Equal(365, vm.QuarantineDays);
    }
}
