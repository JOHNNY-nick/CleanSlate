using CleanSlate.Presentation;

namespace CleanSlate.App;

/// <summary>
/// Development mode of this build (D74). Debug only: a Release build is always
/// <see cref="DevOptions.None"/>, whatever the environment says.
/// </summary>
public static class DevMode
{
    /// <summary>Options read at startup.</summary>
    public static DevOptions Options { get; } =
#if DEBUG
        DevOptions.FromEnvironment(Environment.GetEnvironmentVariable);
#else
        DevOptions.None;
#endif

    /// <summary>
    /// Settings and storage map: <c>%LOCALAPPDATA%\CleanSlate</c>, or <c>%TEMP%\CleanSlate-dev</c>
    /// in simulation, so that the screenshots never change the real settings.
    /// </summary>
    public static string SettingsFolder => Options.Simulation
        ? Path.Combine(Path.GetTempPath(), "CleanSlate-dev")
        : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CleanSlate");
}
