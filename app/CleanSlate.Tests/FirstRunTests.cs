using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>First launch: the user chooses how long the quarantine keeps items (EF-19, D62).</summary>
public sealed class FirstRunTests
{
    private static (ISettingsService Settings, IDialogService Dialogs) Fakes(bool done, int? answer)
    {
        var settings = Substitute.For<ISettingsService>();
        settings.FirstRunDone.Returns(done);
        settings.QuarantineDays.Returns(30);
        var dialogs = Substitute.For<IDialogService>();
        dialogs.ChooseQuarantineDaysAsync(Arg.Any<int>()).Returns(answer);
        return (settings, dialogs);
    }

    [Fact]
    public async Task FirstLaunch_AsksTheDelay_StartingFromTheCurrentOne_AndSavesIt()
    {
        var (settings, dialogs) = Fakes(done: false, answer: 7);
        await FirstRun.RunAsync(settings, dialogs);
        await dialogs.Received(1).ChooseQuarantineDaysAsync(30);
        settings.Received(1).QuarantineDays = 7;
        settings.Received(1).FirstRunDone = true;
    }

    [Fact]
    public async Task LaterLaunches_AskNothing()
    {
        var (settings, dialogs) = Fakes(done: true, answer: 7);
        await FirstRun.RunAsync(settings, dialogs);
        await dialogs.DidNotReceive().ChooseQuarantineDaysAsync(Arg.Any<int>());
        settings.DidNotReceive().QuarantineDays = Arg.Any<int>();
    }

    [Fact]
    public async Task NoAnswer_KeepsTheDelay_AndAsksAgainNextTime()
    {
        var (settings, dialogs) = Fakes(done: false, answer: null);
        await FirstRun.RunAsync(settings, dialogs);
        settings.DidNotReceive().QuarantineDays = Arg.Any<int>();
        settings.DidNotReceive().FirstRunDone = Arg.Any<bool>();
    }

    [Theory]
    [InlineData(0, 1)]
    [InlineData(-5, 1)]
    [InlineData(1000, 365)]
    public async Task TheDelay_StaysBetweenOneAnd365Days(int answer, int saved)
    {
        var (settings, dialogs) = Fakes(done: false, answer: answer);
        await FirstRun.RunAsync(settings, dialogs);
        settings.Received(1).QuarantineDays = saved;
    }

    [Fact]
    public void Choices_IncludeTheDefault_AndAreSorted()
    {
        Assert.Contains(FirstRun.DefaultDays, FirstRun.Choices);
        Assert.Equal(FirstRun.Choices.Order(), FirstRun.Choices);
        Assert.All(FirstRun.Choices, d => Assert.InRange(d, 1, 365));
    }
}
