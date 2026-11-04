using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using NSubstitute.ExceptionExtensions;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>HomeViewModel with a substituted IEngineService (no DLL, no UI).</summary>
public sealed class HomeViewModelTests
{
    [Fact]
    public async Task Load_ShowsEngineVersion()
    {
        var engine = Substitute.For<IEngineService>();
        engine.GetVersionAsync(Arg.Any<CancellationToken>()).Returns("1.2.3");
        var viewModel = new HomeViewModel(engine);

        await viewModel.LoadCommand.ExecuteAsync(null);

        Assert.Equal("1.2.3", viewModel.EngineVersion);
        Assert.False(viewModel.HasError);
    }

    [Fact]
    public async Task Load_EngineFailure_ShowsError()
    {
        var engine = Substitute.For<IEngineService>();
        engine.GetVersionAsync(Arg.Any<CancellationToken>())
            .ThrowsAsync(new EngineException("INTERNAL_PANIC", "boom"));
        var viewModel = new HomeViewModel(engine);

        await viewModel.LoadCommand.ExecuteAsync(null);

        Assert.True(viewModel.HasError);
        Assert.Contains("Erreur interne du moteur", viewModel.ErrorDetails); // French, never the code (D82)
        Assert.Equal(string.Empty, viewModel.EngineVersion);
    }

    private static InstalledApp App(string name, HiddenReason? hidden = null) => new(
        name, name, null, "1.0", null, null, null, null, null, false, null,
        RegistryView.Hklm64, AppKind.Win32, hidden, InstallerKind.Unknown);

    private static Session SessionOf(ulong freed) => new(
        Guid.NewGuid(), DateTimeOffset.Now, Origin.Orphans, [], freed, [], SessionStatus.Completed);

    [Fact]
    public async Task Load_CountsVisibleApps_KeepsThreeSessions_AndSumsFreedSpace()
    {
        var engine = Substitute.For<IEngineService>();
        engine.GetVersionAsync(Arg.Any<CancellationToken>()).Returns("0.1.0");
        engine.ListAppsAsync(Arg.Any<CancellationToken>())
            .Returns([App("A"), App("B"), App("C", HiddenReason.SystemComponent)]);
        engine.ListSessionsAsync(Arg.Any<CancellationToken>())
            .Returns([SessionOf(1024), SessionOf(1024), SessionOf(1024), SessionOf(1024)]);
        var viewModel = new HomeViewModel(engine);

        await viewModel.LoadCommand.ExecuteAsync(null);

        Assert.Equal(2, viewModel.AppCount);
        Assert.Equal(3, viewModel.RecentSessions.Count);
        Assert.False(viewModel.HasNoSession);
        Assert.Equal(Formats.Size(4096), viewModel.FreedText);
        Assert.Equal(Formats.Unknown, viewModel.RecoverableText);
    }

    [Fact]
    public void AnalyzeLeftovers_OpensOrphansAndStartsTheScan()
    {
        var navigation = Substitute.For<INavigationService>();
        var viewModel = new HomeViewModel(Substitute.For<IEngineService>(), navigation: navigation);

        viewModel.AnalyzeLeftoversCommand.Execute(null);

        navigation.Received(1).OpenOrphans(true);
    }

    [Fact]
    public void NotElevated_ShowsTheElevationWarning()
    {
        var viewModel = new HomeViewModel(Substitute.For<IEngineService>(), isElevated: false);

        Assert.True(viewModel.NeedsElevation);
    }
}
