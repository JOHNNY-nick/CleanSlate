using CleanSlate.Interop;
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
        Assert.Contains("INTERNAL_PANIC", viewModel.ErrorDetails);
        Assert.Equal(string.Empty, viewModel.EngineVersion);
    }
}
