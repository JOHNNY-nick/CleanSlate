using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using NSubstitute.ExceptionExtensions;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>OrphansViewModel with a substituted IEngineService (EF-08 to EF-11, ENF-04).</summary>
public sealed class OrphansViewModelTests
{
    private static Leftover Item(string location, Confidence confidence, ulong size) =>
        new(location, LeftoverKind.Folder, location, size, null, confidence, ["raison"], "app");

    private static ScanReport Report(bool cancelled = false, int errors = 0) => new(
        Groups:
        [
            new LeftoverGroup("Audacity", null, [Item(@"C:\A\audacity", Confidence.High, 400_000_000), Item(@"C:\B\audacity", Confidence.Medium, 1_000)], 400_001_000),
            new LeftoverGroup("Data", null, [Item(@"C:\C\data", Confidence.Low, 10)], 10),
        ],
        Errors: Enumerable.Range(0, errors).Select(i => new ItemError("ACCESS_DENIED", "denied", $"C:\\{i}")).ToList(),
        DurationMs: 7_600,
        Cancelled: cancelled);

    internal static ISettingsService Settings(bool simulation = true)
    {
        var settings = Substitute.For<ISettingsService>();
        settings.SimulationMode.Returns(simulation);
        settings.QuarantineDays.Returns(30);
        settings.Exclusions.Returns([]);
        return settings;
    }

    internal static IDialogService Dialogs(bool confirm = true, string? word = null)
    {
        var dialogs = Substitute.For<IDialogService>();
        dialogs.ConfirmCleanAsync(Arg.Any<CleanSummary>()).Returns(new CleanDecision(confirm, word));
        dialogs.ConfirmPurgeAsync(Arg.Any<int>()).Returns(confirm);
        return dialogs;
    }

    private static OrphansViewModel Create(IEngineService engine, IDialogService? dialogs = null, ISettingsService? settings = null, INavigationService? navigation = null) =>
        new(engine, dialogs ?? Dialogs(), settings ?? Settings(), navigation ?? Substitute.For<INavigationService>());

    private static async Task<OrphansViewModel> ScannedAsync(ScanReport report)
    {
        var engine = Substitute.For<IEngineService>();
        engine.ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(report);
        var vm = Create(engine);
        await vm.ScanCommand.ExecuteAsync(null);
        return vm;
    }

    [Fact]
    public async Task Scan_PrechecksOnlyHighConfidence()
    {
        var vm = await ScannedAsync(Report());

        Assert.Equal(2, vm.GroupCount);
        Assert.True(vm.Groups[0].Items[0].IsChecked);
        Assert.False(vm.Groups[0].Items[1].IsChecked);
        Assert.False(vm.Groups[1].Items[0].IsChecked);
        Assert.Equal(1, vm.SelectedCount);
        Assert.Equal("381 Mo", vm.SelectedSizeText);
        Assert.Equal(8, vm.DurationSeconds);
        Assert.True(vm.Groups[0].IsExpanded);
        Assert.False(vm.Groups[1].IsExpanded);
        Assert.False(vm.IsScanning);
        Assert.False(vm.ShowIntro);
    }

    [Fact]
    public async Task Checking_UpdatesSelectionTotals()
    {
        var vm = await ScannedAsync(Report());

        vm.Groups[1].Items[0].IsChecked = true;

        Assert.Equal(2, vm.SelectedCount);
        vm.Groups[0].Items[0].IsChecked = false;
        Assert.Equal(1, vm.SelectedCount);
        Assert.Equal("10 octets", vm.SelectedSizeText);
    }

    [Fact]
    public async Task ConfidenceFilter_HidesOtherItemsAndEmptyGroups()
    {
        var vm = await ScannedAsync(Report());

        vm.FilterIndex = 3; // Low
        Assert.False(vm.Groups[0].IsVisible);
        Assert.True(vm.Groups[1].IsVisible);

        vm.FilterIndex = 2; // Medium
        Assert.True(vm.Groups[0].IsVisible);
        Assert.False(vm.Groups[0].Items[0].IsVisible);
        Assert.True(vm.Groups[0].Items[1].IsVisible);

        vm.FilterIndex = 0;
        Assert.All(vm.Groups.SelectMany(g => g.Items), i => Assert.True(i.IsVisible));
    }

    [Fact]
    public async Task Simulate_ReportsWithoutTouchingAnything()
    {
        var vm = await ScannedAsync(Report());

        Assert.True(vm.SimulateCommand.CanExecute(null));
        Assert.True(vm.QuarantineCommand.CanExecute(null)); // enabled since J4, always behind the confirmation dialog
        vm.SimulateCommand.Execute(null);

        Assert.True(vm.ShowSimulation);
        Assert.Equal(1, vm.SimulatedCount);
        Assert.Equal("381 Mo", vm.SimulatedSizeText);

        vm.Groups[0].Items[0].IsChecked = false;
        Assert.False(vm.ShowSimulation);
        Assert.False(vm.SimulateCommand.CanExecute(null));
    }

    [Fact]
    public async Task CancelledScan_KeepsPartialResults()
    {
        var vm = await ScannedAsync(Report(cancelled: true, errors: 2));

        Assert.True(vm.WasCancelled);
        Assert.Equal(2, vm.ErrorCount);
        Assert.Equal(2, vm.GroupCount);
    }

    [Fact]
    public async Task EmptyScan_ShowsEmptyState()
    {
        var vm = await ScannedAsync(new ScanReport([], [], 10, false));

        Assert.True(vm.ShowEmpty);
        Assert.Equal(0, vm.SelectedCount);
    }

    [Fact]
    public async Task EngineFailure_ShowsError()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .ThrowsAsync(new EngineException("BUSY", "busy"));
        var vm = Create(engine);

        await vm.ScanCommand.ExecuteAsync(null);

        Assert.True(vm.HasError);
        Assert.Contains("BUSY", vm.ErrorDetails);
        Assert.False(vm.IsScanning);
    }

    [Fact]
    public async Task Scan_SendsSimulationOptions()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(Report());
        var vm = Create(engine);

        await vm.ScanCommand.ExecuteAsync(null);

        await engine.Received(1).ScanOrphansAsync(
            Arg.Is<ScanOptions>(o => o.DryRun),
            Arg.Any<IProgress<ScanProgress>?>(),
            Arg.Any<CancellationToken>());
    }

    [Fact]
    public void DeadTasksAndServices_CanNeverBeChecked()
    {
        var service = new LeftoverItemViewModel(new Leftover(
            "s1", LeftoverKind.Service, @"HKLM\SYSTEM\CurrentControlSet\Services\Old", 0, null, Confidence.Medium, ["Exécutable introuvable"], "Old"));
        var task = new LeftoverItemViewModel(Item(@"C:\A\task", Confidence.High, 0) with { Kind = LeftoverKind.ScheduledTask });
        var folder = new LeftoverItemViewModel(Item(@"C:\A\old", Confidence.Medium, 10));

        service.IsChecked = true;
        folder.IsChecked = true;

        Assert.False(service.IsSelectable);
        Assert.False(service.IsChecked);
        Assert.False(task.IsChecked);   // even if the engine ever said High
        Assert.True(folder.IsSelectable);
        Assert.True(folder.IsChecked);
    }

    [Fact]
    public async Task StateActions_NavigateAndListIgnoredItems()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(Report(cancelled: true, errors: 2));
        var navigation = Substitute.For<INavigationService>();
        var vm = Create(engine, navigation: navigation);
        await vm.ScanCommand.ExecuteAsync(null);

        // « Détails »: the ignored items, hidden until asked.
        Assert.Equal([@"C:\0", @"C:\1"], vm.ItemErrors);
        Assert.False(vm.ShowItemErrors);
        vm.ToggleItemErrorsCommand.Execute(null);
        Assert.True(vm.ShowItemErrors);

        // « Retour à l'accueil », « Voir les fichiers », « Désactiver la simulation ».
        vm.GoHomeCommand.Execute(null);
        vm.OpenHistoryCommand.Execute(null);
        vm.OpenSettingsCommand.Execute(null);
        navigation.Received(1).OpenHome();
        navigation.Received(1).OpenHistory();
        navigation.Received(1).OpenSettings();

        // « Relancer »: a new scan, details closed again.
        await vm.ScanCommand.ExecuteAsync(null);
        await engine.Received(2).ScanOrphansAsync(Arg.Any<ScanOptions>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>());
        Assert.False(vm.ShowItemErrors);
    }
}
