using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using Microsoft.Win32;

namespace CleanSlate.App.Services;

/// <summary>
/// <see cref="IFolderService"/> on this PC (D63): known folders through
/// <c>SHGetKnownFolderPath</c> (also in another language or redirected to OneDrive), drive type
/// and file system, links on the path, and the folder window of the Windows App SDK.
/// </summary>
public sealed class FolderService : IFolderService
{
    private static readonly Guid LocalAppDataLow = new("A520A1A4-1780-4FF6-BD18-167343C5AF16");
    private static readonly Guid Downloads = new("374DE290-123F-4565-9164-39C4925E467B");

    /// <inheritdoc/>
    public string DefaultQuarantineFolder { get; } =
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CleanSlate");

    /// <inheritdoc/>
    public string DefaultExportFolder { get; } = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);

    /// <inheritdoc/>
    public FolderProblem CheckQuarantineFolder(string path) => FolderRules.CheckQuarantine(path, Facts(path), Places());

    /// <inheritdoc/>
    public FolderProblem CheckExportFolder(string path) => FolderRules.CheckExport(path, Facts(path), Places());

    /// <inheritdoc/>
    public void PrepareQuarantineFolder(string path)
    {
        if (FolderRules.SamePath(path, DefaultQuarantineFolder))
        {
            Directory.CreateDirectory(path);
            return;
        }

        // Checked again just before creating: the folder may have changed since it was chosen.
        if (CheckQuarantineFolder(path) is var problem and not FolderProblem.None)
        {
            throw new IOException($"Dossier refusé : {problem}");
        }

        string? parent = Path.GetDirectoryName(path);
        if (parent is not null)
        {
            Directory.CreateDirectory(parent);
        }

        var folder = new DirectoryInfo(path);
        if (folder.Exists)
        {
            folder.SetAccessControl(ProtectedSecurity());
        }
        else
        {
            folder.Create(ProtectedSecurity());
        }
    }

    /// <inheritdoc/>
    public async Task<string?> PickFolderAsync()
    {
        var picker = new Microsoft.Windows.Storage.Pickers.FolderPicker(App.GetService<MainWindow>().AppWindow.Id);
        var result = await picker.PickSingleFolderAsync();
        return result?.Path;
    }

    // SYSTEM and Administrators only, inherited by everything inside: a program running without
    // administrator rights can neither read nor alter the quarantine (CleanSlate runs elevated).
    private static DirectorySecurity ProtectedSecurity()
    {
        var security = new DirectorySecurity();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
        foreach (var sid in new[] { WellKnownSidType.LocalSystemSid, WellKnownSidType.BuiltinAdministratorsSid })
        {
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(sid, null),
                FileSystemRights.FullControl,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
        }

        security.SetOwner(new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null));
        return security;
    }

    private static FolderFacts Facts(string path)
    {
        if (FolderRules.Normalize(path) is not string full)
        {
            return new FolderFacts(false, false, null, false, false, true);
        }

        bool network = full.StartsWith(@"\\", StringComparison.Ordinal);
        bool fixedDrive = false;
        string? fileSystem = null;
        if (!network && Path.GetPathRoot(full) is string root)
        {
            try
            {
                var drive = new DriveInfo(root);
                network = drive.DriveType == DriveType.Network;
                fixedDrive = drive.DriveType == DriveType.Fixed;
                fileSystem = drive.IsReady ? drive.DriveFormat : null;
            }
            catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException)
            {
                // Unknown drive: refused as not a local disk.
            }
        }

        bool exists = !network && Directory.Exists(full);
        bool empty = true;
        if (exists)
        {
            try
            {
                empty = !Directory.EnumerateFileSystemEntries(full).Any();
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                empty = false;
            }
        }

        return new FolderFacts(network, fixedDrive, fileSystem, !network && HasLink(full), exists, empty);
    }

    // The folder or any existing parent is a symbolic link or a junction (safety rule 6).
    private static bool HasLink(string full)
    {
        for (var dir = new DirectoryInfo(full); dir is not null; dir = dir.Parent)
        {
            try
            {
                if (dir.Exists && dir.Attributes.HasFlag(FileAttributes.ReparsePoint))
                {
                    return true;
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                return true;
            }
        }

        return false;
    }

    private FolderPlaces Places()
    {
        string profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var system = new[]
        {
            Environment.GetFolderPath(Environment.SpecialFolder.Windows),
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        };
        var scanned = new[]
        {
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            KnownFolder(LocalAppDataLow) ?? Path.Combine(profile, "AppData", "LocalLow"),
            Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments),
            KnownFolder(Downloads) ?? Path.Combine(profile, "Downloads"),
        };
        return new FolderPlaces(
            system.Where(p => p.Length > 0).ToList(),
            scanned.Where(p => p.Length > 0).ToList(),
            CloudFolders(),
            DefaultQuarantineFolder);
    }

    // OneDrive (personal and work) and every folder declared by a Windows sync engine.
    private static List<string> CloudFolders()
    {
        var folders = new List<string>();
        foreach (string variable in new[] { "OneDrive", "OneDriveConsumer", "OneDriveCommercial" })
        {
            if (Environment.GetEnvironmentVariable(variable) is { Length: > 0 } value)
            {
                folders.Add(value);
            }
        }

        try
        {
            using var providers = Registry.CurrentUser.OpenSubKey(@"Software\SyncEngines\Providers");
            foreach (string provider in providers?.GetSubKeyNames() ?? [])
            {
                using var key = providers!.OpenSubKey(provider);
                foreach (string mount in key?.GetSubKeyNames() ?? [])
                {
                    using var entry = key!.OpenSubKey(mount);
                    if (entry?.GetValue("MountPoint") is string { Length: > 0 } point)
                    {
                        folders.Add(point);
                    }
                }
            }
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException or IOException)
        {
            // The environment variables still cover OneDrive.
        }

        return folders;
    }

#pragma warning disable SYSLIB1054 // LibraryImport needs unsafe code (as in CrashHandler).
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetKnownFolderPath(ref Guid folderId, uint flags, IntPtr token, out IntPtr path);
#pragma warning restore SYSLIB1054

    private static string? KnownFolder(Guid id)
    {
        int result = SHGetKnownFolderPath(ref id, 0, IntPtr.Zero, out IntPtr pointer);
        try
        {
            return result == 0 ? Marshal.PtrToStringUni(pointer) : null;
        }
        finally
        {
            Marshal.FreeCoTaskMem(pointer);
        }
    }
}
