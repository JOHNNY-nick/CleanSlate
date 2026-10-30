using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Uninstall wizard (EF-12 to EF-15, ENF-07: 5 clicks).</summary>
public sealed class UninstallWizardViewModelTests
{
    private static readonly InstalledApp Gimp = new(
        "g1", "GIMP 2.10.38", "The GIMP Team", "2.10.38", null, @"C:\Program Files\GIMP 2", 421_888,
        "\"C:\\Program Files\\GIMP 2\\uninst\\unins000.exe\"", "\"C:\\Program Files\\GIMP 2\\uninst\\unins000.exe\" /SILENT",
        false, null, RegistryView.Hklm64, AppKind.Win32, null, InstallerKind.InnoSetup);

    private static UninstallResult Result(bool simulated = false, bool stillInstalled = false) =>
        new("g1", "GIMP 2.10.38", "\"C:\\…\\unins000.exe\"", simulated, true, null, 0, false, false, stillInstalled);

    private static readonly Leftover Folder = new("l1", LeftoverKind.Folder, @"C:\Program Files\GIMP 2", 5_000, null, Confidence.High, ["Ancien dossier d'installation de l'application"], "GIMP 2.10.38");

    private static (UninstallWizardViewModel Vm, IEngineService Engine, IDialogService Dialogs) Create(bool simulation = false)
    {
        var engine = Substitute.For<IEngineService>();
        var dialogs = OrphansViewModelTests.Dialogs();
        var vm = new UninstallWizardViewModel(engine, dialogs, OrphansViewModelTests.Settings(simulation), Substitute.For<INavigationService>());
        return (vm, engine, dialogs);
    }

    [Fact]
    public async Task FullUninstall_InFiveClicks()
    {
        // Clicks 1 and 2 (select the row, « Désinstaller complètement ») open the wizard.
        var (vm, engine, dialogs) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>()).Returns(Result());
        engine.ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new ScanReport([new LeftoverGroup("GIMP 2.10.38", "The GIMP Team", [Folder], 5_000)], [], 10, false));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>()).Returns(new Session(
            Guid.NewGuid(), DateTimeOffset.Now, Origin.ForApp("g1"),
            [new QuarantinedItem("l1", LeftoverKind.Folder, Folder.Location, @"C:\Q", 5_000, null, null, DateTimeOffset.Now, null, QuarantineStatus.Quarantined, null)],
            5_000, [], SessionStatus.Completed, false));
        vm.Start(Gimp, complete: true);
        Assert.True(vm.IsStep1);
        Assert.True(vm.CreateRestorePoint);
        Assert.True(vm.CanQuiet);

        await vm.LaunchCommand.ExecuteAsync(null);      // click 3: « Lancer la désinstallation »
        Assert.True(vm.IsStep3);
        Assert.Equal(1, vm.SelectedCount);
        await vm.QuarantineCommand.ExecuteAsync(null);  // click 4 + click 5 (dialog confirmed)

        Assert.True(vm.IsStep4);
        Assert.Equal(1, vm.CleanedCount);
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.AppId == "g1" && r.CreateRestorePoint && !r.DryRun && !r.Quiet), Arg.Any<CancellationToken>());
        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => p.Origin.Kind == "app" && p.Origin.AppId == "g1" && !p.DryRun), Arg.Any<CancellationToken>());
        await dialogs.Received(1).ConfirmCleanAsync(Arg.Any<CleanSummary>());
    }

    [Fact]
    public async Task CancelledUninstaller_SkipsLeftovers()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>()).Returns(Result(stillInstalled: true));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.True(vm.StillInstalled);
        await engine.DidNotReceive().ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task NoLeftover_SkipsStepThree()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>()).Returns(Result());
        engine.ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new ScanReport([], [], 1, false));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
    }

    [Fact]
    public async Task ClassicUninstall_StopsAfterTheUninstaller_AndQuietIsSent()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>()).Returns(Result());
        vm.Start(Gimp, complete: false);
        vm.Quiet = true;
        vm.CreateRestorePoint = false;

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.Quiet && !r.CreateRestorePoint), Arg.Any<CancellationToken>());
        await engine.DidNotReceive().ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Simulation_OnlyReportsTheCommand()
    {
        var (vm, engine, _) = Create(simulation: true);
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>()).Returns(Result(simulated: true, stillInstalled: true));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.False(vm.StillInstalled);
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.DryRun), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task MissingUninstaller_ShowsTheErrorOnStepOne()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>())
            .Returns<Task<UninstallResult>>(_ => throw new EngineException("NO_UNINSTALLER", "missing"));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep1);
        Assert.Equal("NO_UNINSTALLER", vm.ErrorCode);
        Assert.True(vm.HasError);
        Assert.True(vm.IsUninstallerMissing);
        Assert.False(vm.ShowGenericError);
        Assert.True(vm.SearchLeftoversAnywayCommand.CanExecute(null));
    }

    [Fact]
    public async Task MissingUninstaller_SearchesLeftoversAnyway_NothingPrechecked()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>())
            .Returns<Task<UninstallResult>>(_ => throw new EngineException("NO_UNINSTALLER", "missing"));
        // The engine caps these leftovers at Medium (D45).
        Leftover medium = Folder with { Confidence = Confidence.Medium };
        engine.ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new ScanReport([new LeftoverGroup("GIMP 2.10.38", "The GIMP Team", [medium], 5_000)], [], 10, false));
        engine.CleanAsync(Arg.Any<CleanPlan>(), Arg.Any<CancellationToken>()).Returns(new Session(
            Guid.NewGuid(), DateTimeOffset.Now, Origin.ForApp("g1"), [], 0, [], SessionStatus.Completed, true));
        vm.Start(Gimp, complete: true);
        await vm.LaunchCommand.ExecuteAsync(null);

        await vm.SearchLeftoversAnywayCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep3);
        Assert.False(vm.HasError);
        Assert.True(vm.WithoutUninstaller);
        Assert.False(vm.ShowUninstalledBanner);
        Assert.Equal(0, vm.SelectedCount);
        await engine.Received(1).ScanAppLeftoversAsync(
            Arg.Is<AppLeftoversRequest>(r => r.AppId == "g1" && r.UninstallerMissing),
            Arg.Any<IProgress<ScanProgress>?>(),
            Arg.Any<CancellationToken>());

        vm.Groups[0].Items[0].IsChecked = true;
        await vm.QuarantineCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.False(vm.ShowCommandLine);
        Assert.True(vm.ShowCleanSimulatedLine);
        Assert.False(vm.ShowCleanedLine);
    }

    [Fact]
    public async Task MissingUninstaller_NoLeftover_GoesToTheSummary()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>())
            .Returns<Task<UninstallResult>>(_ => throw new EngineException("NO_UNINSTALLER", "missing"));
        engine.ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new ScanReport([], [], 10, false));
        vm.Start(Gimp, complete: true);
        await vm.LaunchCommand.ExecuteAsync(null);

        await vm.SearchLeftoversAnywayCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.True(vm.NoLeftoverFound);
        Assert.False(vm.ShowCleanedLine);
    }

    [Fact]
    public async Task OtherErrors_CannotSearchAnyway()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<CancellationToken>())
            .Returns<Task<UninstallResult>>(_ => throw new EngineException("UNSAFE_UNINSTALLER", "shell"));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.ShowGenericError);
        Assert.False(vm.IsUninstallerMissing);
        Assert.False(vm.SearchLeftoversAnywayCommand.CanExecute(null));
    }
}
