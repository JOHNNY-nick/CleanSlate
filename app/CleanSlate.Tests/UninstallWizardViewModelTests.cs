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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result());
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
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.AppId == "g1" && r.CreateRestorePoint && !r.DryRun && !r.Quiet), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>());
        await engine.Received(1).CleanAsync(Arg.Is<CleanPlan>(p => p.Origin.Kind == "app" && p.Origin.AppId == "g1" && !p.DryRun), Arg.Any<CancellationToken>());
        await dialogs.Received(1).ConfirmCleanAsync(Arg.Any<CleanSummary>());
    }

    [Fact]
    public async Task CancelledUninstaller_SkipsLeftovers()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result(stillInstalled: true));
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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result());
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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result());
        vm.Start(Gimp, complete: false);
        vm.Quiet = true;
        vm.CreateRestorePoint = false;

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.Quiet && !r.CreateRestorePoint), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>());
        await engine.DidNotReceive().ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Simulation_OnlyReportsTheCommand()
    {
        var (vm, engine, _) = Create(simulation: true);
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result(simulated: true, stillInstalled: true));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.False(vm.StillInstalled);
        await engine.Received(1).UninstallAsync(Arg.Is<UninstallRequest>(r => r.DryRun), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task MissingUninstaller_ShowsTheErrorOnStepOne()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
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
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
            .Returns<Task<UninstallResult>>(_ => throw new EngineException("UNSAFE_UNINSTALLER", "shell"));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.ShowGenericError);
        Assert.False(vm.IsUninstallerMissing);
        Assert.False(vm.SearchLeftoversAnywayCommand.CanExecute(null));
    }

    [Fact]
    public async Task Progress_ShowsTimedSteps_RealPercentOrJobProcesses()
    {
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(call =>
            {
                var progress = call.Arg<IProgress<UninstallProgress>?>()!;
                progress.Report(new UninstallProgress(UninstallStep.RestorePoint, 10, null, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Launching, 900, null, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 1_200, null, null, [new JobProcess(4242, "unins000.exe")]));
                // .exe uninstaller: never a percentage, the Job Object instead.
                Assert.True(vm.IsProgressIndeterminate);
                Assert.Equal(["unins000.exe (PID 4242)"], vm.Processes);
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 72_000, 40, "Suppression des fichiers", []));
                Assert.False(vm.IsProgressIndeterminate);
                Assert.Equal(40, vm.ProgressPercent);
                Assert.Equal("Suppression des fichiers", vm.ActionText);
                Assert.Equal("01:12", vm.ElapsedText);
                progress.Report(new UninstallProgress(UninstallStep.Verifying, 73_000, null, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Done, 73_100, null, null, []));
                return Task.FromResult(Result(stillInstalled: true));
            });
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.Equal(
            ["RestorePoint", "Launching", "Waiting", "Verifying", "Done"],
            vm.Steps.Select(s => s.Key));
        Assert.All(vm.Steps, s => Assert.Matches(@"^\d\d:\d\d:\d\d$", s.TimeText));
    }

    [Fact]
    public async Task FastUninstall_KeepsTheTimedHistory_UpToTheSummary()
    {
        // VLC in Windows Sandbox: the MSI uninstall ends in about 1 s, before step 2 can be read.
        var (vm, engine, _) = Create();
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(call =>
            {
                var progress = call.Arg<IProgress<UninstallProgress>?>()!;
                progress.Report(new UninstallProgress(UninstallStep.Launching, 12, null, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 30, 0, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 400, 42.4, "Suppression des fichiers", []));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 450, 42.6, "Suppression des fichiers", []));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 900, 100, null, [new JobProcess(4242, "msiexec.exe")]));
                progress.Report(new UninstallProgress(UninstallStep.Waiting, 1_100, 100, null, [new JobProcess(4242, "msiexec.exe")]));
                progress.Report(new UninstallProgress(UninstallStep.Verifying, 1_295, null, null, []));
                progress.Report(new UninstallProgress(UninstallStep.Done, 1_307, null, null, []));
                return Task.FromResult(Result());
            });
        engine.ScanAppLeftoversAsync(Arg.Any<AppLeftoversRequest>(), Arg.Any<IProgress<ScanProgress>?>(), Arg.Any<CancellationToken>())
            .Returns(new ScanReport([], [], 10, false));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.True(vm.HasHistory);
        // A line per change: step, whole percentage, action or process list (not per event).
        Assert.Equal(
            [
                ("Launching", "00:00.012", ""),
                ("Waiting", "00:00.030", "0 %"),
                ("Waiting", "00:00.400", "42 % — Suppression des fichiers"),
                ("Waiting", "00:00.450", "43 % — Suppression des fichiers"),
                ("Waiting", "00:00.900", "100 % — msiexec.exe (PID 4242)"),
                ("Verifying", "00:01.295", ""),
                ("Done", "00:01.307", ""),
            ],
            vm.History.Select(h => (h.StepKey, h.ElapsedText, h.Detail)));
        Assert.All(vm.History, h => Assert.Matches(@"^\d\d:\d\d:\d\d$", h.TimeText));
        Assert.Equal(8, vm.ProgressEventCount);
        Assert.Equal("100 %", vm.MaxPercentText);
        Assert.Equal("msiexec.exe", vm.ProcessNamesText);
        Assert.Equal("00:01.307", vm.DurationText);
    }

    [Fact]
    public async Task Simulation_SaysInTheHistoryThatNothingWasRun()
    {
        // Complements scenario of 30 Oct. 2026: CleanSlate in simulation (default of a new
        // Sandbox), the summary came at once and looked like a finished uninstall.
        var (vm, engine, _) = Create(simulation: true);
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result(simulated: true, stillInstalled: true));
        vm.Start(Gimp, complete: true);

        await vm.LaunchCommand.ExecuteAsync(null);

        Assert.True(vm.IsStep4);
        Assert.True(vm.WasSimulated);
        var line = Assert.Single(vm.History);
        Assert.Equal("Simulated", line.StepKey);
        Assert.Equal("\"C:\\…\\unins000.exe\"", line.Detail);
        Assert.Equal(0, vm.ProgressEventCount);
        Assert.Equal("—", vm.MaxPercentText);
        Assert.Equal("—", vm.ProcessNamesText);
    }

    [Fact]
    public async Task ANewUninstall_StartsAFreshHistory()
    {
        var (vm, engine, _) = Create(simulation: true);
        engine.UninstallAsync(Arg.Any<UninstallRequest>(), Arg.Any<IProgress<UninstallProgress>?>(), Arg.Any<CancellationToken>()).Returns(Result(simulated: true, stillInstalled: true));
        vm.Start(Gimp, complete: true);
        await vm.LaunchCommand.ExecuteAsync(null);

        vm.Start(Gimp, complete: true);

        Assert.False(vm.HasHistory);
        Assert.Equal(0, vm.ProgressEventCount);
    }

    [Fact]
    public void StoreApplication_HasNoRestorePoint()
    {
        var (vm, _, _) = Create();
        vm.Start(Gimp with { Kind = AppKind.Store, PackageFullName = "Vendor.App_1.0.0.0_x64__abcdefghjk123" }, complete: true);
        Assert.False(vm.IsWin32);
        vm.Start(Gimp, complete: true);
        Assert.True(vm.IsWin32);
    }
}