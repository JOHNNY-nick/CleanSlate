using System.Diagnostics;

namespace CleanSlate.App.Services;

/// <summary>
/// The single way the application starts a program (D82): the Explorer, the Properties
/// dialog, CleanSlate itself with administrator rights. A console program started from here
/// never opens a console window (<see cref="ProcessStartInfo.CreateNoWindow"/>); the engine
/// does the same on its side (<c>process::hidden</c>).
/// </summary>
internal static class Launcher
{
    /// <summary>Starts <paramref name="info"/> without a console window.</summary>
    public static void Start(ProcessStartInfo info)
    {
        info.CreateNoWindow = true;
        using Process? _ = Process.Start(info);
    }

    /// <summary>Opens the Explorer of Windows with <paramref name="arguments"/>.</summary>
    public static void Explorer(params string[] arguments)
    {
        var info = new ProcessStartInfo(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "explorer.exe")) { UseShellExecute = false };
        foreach (string argument in arguments)
        {
            info.ArgumentList.Add(argument);
        }

        Start(info);
    }
}
