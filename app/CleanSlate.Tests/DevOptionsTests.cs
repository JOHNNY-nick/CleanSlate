using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Development mode (D74): a pure function, never tested by launching the application.</summary>
public sealed class DevOptionsTests
{
    private static Func<string, string?> Env(params (string Name, string Value)[] values) =>
        name => values.FirstOrDefault(v => v.Name == name).Value;

    [Fact]
    public void WithoutVariables_TheApplicationActsNormally()
    {
        DevOptions options = DevOptions.FromEnvironment(Env());
        Assert.False(options.Simulation);
        Assert.False(options.EngineDryRun);
        Assert.Null(options.CaptureDir);
    }

    [Fact]
    public void Simulation_CreatesTheEngineInDryRun()
    {
        DevOptions options = DevOptions.FromEnvironment(Env((DevOptions.SimulationVariable, "1")));
        Assert.True(options.EngineDryRun);
    }

    [Theory]
    [InlineData("0")]
    [InlineData("true")]
    [InlineData("")]
    public void Screenshots_NeedTheSimulation(string simulation)
    {
        DevOptions options = DevOptions.FromEnvironment(Env((DevOptions.SimulationVariable, simulation), (DevOptions.CaptureVariable, @"C:\captures")));
        Assert.False(options.Simulation);
        Assert.Null(options.CaptureDir);
    }

    [Fact]
    public void Screenshots_InSimulation_GoToTheFolderGiven()
    {
        DevOptions options = DevOptions.FromEnvironment(Env((DevOptions.SimulationVariable, "1"), (DevOptions.CaptureVariable, @"C:\captures")));
        Assert.Equal(@"C:\captures", options.CaptureDir);
        Assert.True(options.EngineDryRun);
    }
}
