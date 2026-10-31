namespace CleanSlate.Presentation;

/// <summary>Why a folder chosen by the user is refused (D63); <see cref="None"/> when it is accepted.</summary>
public enum FolderProblem
{
    /// <summary>Accepted.</summary>
    None,

    /// <summary>Not a full path such as <c>D:\CleanSlate</c>.</summary>
    NotAbsolute,

    /// <summary>Network share or mapped network drive.</summary>
    Network,

    /// <summary>Removable, optical or unknown drive.</summary>
    NotLocalDisk,

    /// <summary>File system without permissions (FAT32, exFAT).</summary>
    NotNtfs,

    /// <summary>The root of a drive itself.</summary>
    DriveRoot,

    /// <summary>Windows, Program Files, ProgramData, recycle bin or system volume folder.</summary>
    SystemFolder,

    /// <summary>A folder CleanSlate scans for leftovers (AppData, Documents) or that Windows empties (Downloads).</summary>
    ScannedFolder,

    /// <summary>Synchronised to the cloud (OneDrive…): removed data would leave the PC.</summary>
    CloudSynced,

    /// <summary>The path goes through a symbolic link or a junction (safety rule 6).</summary>
    Link,

    /// <summary>The folder already holds files.</summary>
    NotEmpty,
}

/// <summary>What the system says about a chosen folder (gathered by the App layer).</summary>
/// <param name="IsNetwork">UNC path or mapped network drive.</param>
/// <param name="IsFixedDrive">Internal disk (not removable, optical or RAM).</param>
/// <param name="FileSystem">NTFS, ReFS, FAT32…; null when the drive is not ready.</param>
/// <param name="HasLink">The folder or one of its parents is a symbolic link or a junction.</param>
/// <param name="Exists">The folder exists.</param>
/// <param name="IsEmpty">The folder holds nothing (true when it does not exist).</param>
public sealed record FolderFacts(bool IsNetwork, bool IsFixedDrive, string? FileSystem, bool HasLink, bool Exists, bool IsEmpty);

/// <summary>Known folders of this PC, from <c>SHGetKnownFolderPath</c> (other languages, OneDrive redirection).</summary>
/// <param name="System">Windows, Program Files, Program Files (x86), ProgramData.</param>
/// <param name="Scanned">AppData (Roaming, Local, LocalLow), Documents, Downloads.</param>
/// <param name="Cloud">Folders synchronised by OneDrive or another sync engine.</param>
/// <param name="DefaultQuarantine">Default data folder, <c>%LOCALAPPDATA%\CleanSlate</c>.</param>
public sealed record FolderPlaces(
    IReadOnlyList<string> System,
    IReadOnlyList<string> Scanned,
    IReadOnlyList<string> Cloud,
    string DefaultQuarantine);

/// <summary>
/// Rules for the folders the user chooses (D63). The quarantine keeps what CleanSlate removed,
/// to restore it identically: a local NTFS or ReFS disk, never a scanned, system or synchronised
/// folder, never behind a link, empty unless it is the default folder. The export folder only
/// avoids system folders and links: a USB stick or a network share is fine for a zip of logs.
/// </summary>
public static class FolderRules
{
    /// <summary>Checks a quarantine folder.</summary>
    public static FolderProblem CheckQuarantine(string path, FolderFacts facts, FolderPlaces places)
    {
        if (Normalize(path) is not string full)
        {
            return FolderProblem.NotAbsolute;
        }

        if (full.StartsWith(@"\\", StringComparison.Ordinal) || facts.IsNetwork)
        {
            return FolderProblem.Network;
        }

        if (!facts.IsFixedDrive)
        {
            return FolderProblem.NotLocalDisk;
        }

        if (!string.Equals(facts.FileSystem, "NTFS", StringComparison.OrdinalIgnoreCase)
            && !string.Equals(facts.FileSystem, "ReFS", StringComparison.OrdinalIgnoreCase))
        {
            return FolderProblem.NotNtfs;
        }

        if (IsDriveRoot(full))
        {
            return FolderProblem.DriveRoot;
        }

        if (IsSystem(full, places))
        {
            return FolderProblem.SystemFolder;
        }

        bool isDefault = SamePath(full, places.DefaultQuarantine);
        if (!isDefault && places.Scanned.Any(root => IsUnder(full, root)))
        {
            return FolderProblem.ScannedFolder;
        }

        if (places.Cloud.Any(root => IsUnder(full, root)))
        {
            return FolderProblem.CloudSynced;
        }

        if (facts.HasLink)
        {
            return FolderProblem.Link;
        }

        return !isDefault && facts.Exists && !facts.IsEmpty ? FolderProblem.NotEmpty : FolderProblem.None;
    }

    /// <summary>Checks the folder where the zip of the logs is written.</summary>
    public static FolderProblem CheckExport(string path, FolderFacts facts, FolderPlaces places)
    {
        if (Normalize(path) is not string full)
        {
            return FolderProblem.NotAbsolute;
        }

        if (IsSystem(full, places))
        {
            return FolderProblem.SystemFolder;
        }

        return facts.HasLink ? FolderProblem.Link : FolderProblem.None;
    }

    /// <summary>The two paths name the same folder (case and final backslash ignored).</summary>
    public static bool SamePath(string a, string b) =>
        string.Equals(Trim(a), Trim(b), StringComparison.OrdinalIgnoreCase);

    /// <summary><paramref name="path"/> is <paramref name="root"/> or inside it.</summary>
    public static bool IsUnder(string path, string root)
    {
        string p = Trim(path);
        string r = Trim(root);
        return r.Length > 0
            && (p.Equals(r, StringComparison.OrdinalIgnoreCase)
                || p.StartsWith(r + @"\", StringComparison.OrdinalIgnoreCase));
    }

    /// <summary>Full path without « .. », or null when <paramref name="path"/> is not a full path.</summary>
    public static string? Normalize(string? path)
    {
        if (string.IsNullOrWhiteSpace(path) || !Path.IsPathFullyQualified(path.Trim()))
        {
            return null;
        }

        try
        {
            return Path.GetFullPath(path.Trim());
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return null;
        }
    }

    private static bool IsDriveRoot(string full) =>
        Path.GetPathRoot(full) is string root && SamePath(full, root);

    private static bool IsSystem(string full, FolderPlaces places) =>
        places.System.Any(root => IsUnder(full, root))
        || full.Split('\\').Any(part => part.StartsWith('$')
            || part.Equals("System Volume Information", StringComparison.OrdinalIgnoreCase));

    private static string Trim(string path) => path.Trim().TrimEnd('\\');
}
