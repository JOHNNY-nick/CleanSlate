namespace CleanSlate.Presentation;

/// <summary>
/// Development mode of a Debug build (D74), read from the environment:
/// <c>CLEANSLATE_DEV_SIMULATION=1</c> creates the engine with <c>dry_run = true</c>, so that
/// every action is simulated whatever the request says, skips the purge at startup and keeps
/// settings and the storage map in a folder of its own; <c>CLEANSLATE_CAPTURE_DIR</c> then
/// saves a screenshot of every page in each theme there and closes the application.
/// A Release build never reads these variables.
/// </summary>
/// <param name="Simulation">The engine simulates every action.</param>
/// <param name="CaptureDir">Folder of the screenshots, or null.</param>
/// <param name="CaptureSet">« multicolore »: only the mock-up of the Multicolore styles (D76).</param>
public sealed record DevOptions(bool Simulation, string? CaptureDir, string? CaptureSet = null)
{
    /// <summary>Variable that turns the simulation on.</summary>
    public const string SimulationVariable = "CLEANSLATE_DEV_SIMULATION";

    /// <summary>Variable that names the folder of the screenshots.</summary>
    public const string CaptureVariable = "CLEANSLATE_CAPTURE_DIR";

    /// <summary>Variable that picks a set of screenshots (<c>multicolore</c>).</summary>
    public const string CaptureSetVariable = "CLEANSLATE_CAPTURE_SET";

    /// <summary>Normal application: nothing simulated, no screenshot.</summary>
    public static DevOptions None { get; } = new(false, null);

    /// <summary>
    /// Reads the variables. Screenshots need the simulation: the capture walks every page and
    /// must never be able to act on the PC.
    /// </summary>
    public static DevOptions FromEnvironment(Func<string, string?> variable)
    {
        bool simulation = variable(SimulationVariable) == "1";
        string? capture = variable(CaptureVariable);
        string? folder = simulation && !string.IsNullOrWhiteSpace(capture) ? capture : null;
        return new DevOptions(simulation, folder, folder is null ? null : variable(CaptureSetVariable));
    }

    /// <summary><c>dry_run</c> of the engine configuration: true in simulation.</summary>
    public bool EngineDryRun => Simulation;
}
