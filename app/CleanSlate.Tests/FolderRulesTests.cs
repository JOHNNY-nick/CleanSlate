using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// Folders the user may choose (D63). The quarantine holds what CleanSlate removed: it must stay
/// on a local disk, outside the folders CleanSlate scans, outside Windows and cloud sync, empty,
/// and never behind a link (safety rule 6).
/// </summary>
public sealed class FolderRulesTests
{
    private static readonly FolderPlaces Places = new(
        System: [@"C:\Windows", @"C:\Program Files", @"C:\Program Files (x86)", @"C:\ProgramData"],
        Scanned: [@"C:\Users\Ana\AppData\Roaming", @"C:\Users\Ana\AppData\Local", @"C:\Users\Ana\AppData\LocalLow", @"C:\Users\Ana\Documents", @"C:\Users\Ana\Downloads"],
        Cloud: [@"C:\Users\Ana\OneDrive"],
        DefaultQuarantine: @"C:\Users\Ana\AppData\Local\CleanSlate");

    private static readonly FolderFacts Local = new(IsNetwork: false, IsFixedDrive: true, FileSystem: "NTFS", HasLink: false, Exists: false, IsEmpty: true);

    [Theory]
    [InlineData(@"C:\Users\Ana\AppData\Local\CleanSlate")]
    [InlineData(@"c:\users\ana\appdata\local\cleanslate\")]
    [InlineData(@"D:\CleanSlate\Quarantaine")]
    [InlineData(@"C:\CleanSlate")]
    [InlineData(@"C:\Users\Ana\Quarantaine CleanSlate")]
    public void Quarantine_Accepted(string path) =>
        Assert.Equal(FolderProblem.None, FolderRules.CheckQuarantine(path, Local, Places));

    [Theory]
    [InlineData("", FolderProblem.NotAbsolute)]
    [InlineData(@"CleanSlate", FolderProblem.NotAbsolute)]
    [InlineData(@"D:CleanSlate", FolderProblem.NotAbsolute)]
    [InlineData(@"\\nas\partage\CleanSlate", FolderProblem.Network)]
    [InlineData(@"D:\", FolderProblem.DriveRoot)]
    [InlineData(@"C:\Windows\Temp\CS", FolderProblem.SystemFolder)]
    [InlineData(@"C:\Program Files\CleanSlate\q", FolderProblem.SystemFolder)]
    [InlineData(@"C:\ProgramData\CleanSlate", FolderProblem.SystemFolder)]
    [InlineData(@"D:\$Recycle.Bin\q", FolderProblem.SystemFolder)]
    [InlineData(@"D:\System Volume Information\q", FolderProblem.SystemFolder)]
    [InlineData(@"C:\Users\Ana\AppData\Roaming\CleanSlate", FolderProblem.ScannedFolder)]
    [InlineData(@"C:\Users\Ana\AppData\Local\Temp\q", FolderProblem.ScannedFolder)]
    [InlineData(@"C:\Users\Ana\Documents\Quarantaine", FolderProblem.ScannedFolder)]
    [InlineData(@"C:\Users\Ana\Downloads\q", FolderProblem.ScannedFolder)]
    [InlineData(@"C:\Users\Ana\OneDrive\Quarantaine", FolderProblem.CloudSynced)]
    [InlineData(@"C:\Users\Ana\AppData\Local\CleanSlate\..\..\Roaming\x", FolderProblem.ScannedFolder)]
    public void Quarantine_Refused(string path, FolderProblem expected) =>
        Assert.Equal(expected, FolderRules.CheckQuarantine(path, Local, Places));

    [Fact]
    public void Quarantine_RefusesRemovableNetworkAndNonNtfsDisks_LinksAndFullFolders()
    {
        const string path = @"E:\CleanSlate";
        Assert.Equal(FolderProblem.NotLocalDisk, FolderRules.CheckQuarantine(path, Local with { IsFixedDrive = false }, Places));
        Assert.Equal(FolderProblem.Network, FolderRules.CheckQuarantine(path, Local with { IsNetwork = true }, Places));
        Assert.Equal(FolderProblem.NotNtfs, FolderRules.CheckQuarantine(path, Local with { FileSystem = "FAT32" }, Places));
        Assert.Equal(FolderProblem.None, FolderRules.CheckQuarantine(path, Local with { FileSystem = "ReFS" }, Places));
        Assert.Equal(FolderProblem.Link, FolderRules.CheckQuarantine(path, Local with { HasLink = true }, Places));
        Assert.Equal(FolderProblem.NotEmpty, FolderRules.CheckQuarantine(path, Local with { Exists = true, IsEmpty = false }, Places));
        Assert.Equal(FolderProblem.None, FolderRules.CheckQuarantine(path, Local with { Exists = true, IsEmpty = true }, Places));
        // The default folder already holds CleanSlate's own data: never « not empty ».
        Assert.Equal(FolderProblem.None, FolderRules.CheckQuarantine(Places.DefaultQuarantine, Local with { Exists = true, IsEmpty = false }, Places));
    }

    [Theory]
    [InlineData(@"C:\Users\Ana\Desktop", FolderProblem.None)]
    [InlineData(@"C:\Users\Ana\OneDrive\Bureau", FolderProblem.None)]
    [InlineData(@"\\nas\partage\journaux", FolderProblem.None)]
    [InlineData(@"E:\", FolderProblem.None)]
    [InlineData(@"C:\Users\Ana\Documents", FolderProblem.None)]
    [InlineData("journaux", FolderProblem.NotAbsolute)]
    [InlineData(@"C:\Windows\System32", FolderProblem.SystemFolder)]
    [InlineData(@"C:\Program Files\CleanSlate", FolderProblem.SystemFolder)]
    public void Export(string path, FolderProblem expected) =>
        Assert.Equal(expected, FolderRules.CheckExport(path, Local, Places));

    [Fact]
    public void Export_RefusesLinks() =>
        Assert.Equal(FolderProblem.Link, FolderRules.CheckExport(@"C:\Users\Ana\Desktop", Local with { HasLink = true }, Places));

    [Theory]
    [InlineData(@"C:\A\B", @"C:\A", true)]
    [InlineData(@"C:\A", @"C:\A\", true)]
    [InlineData(@"c:\a\b", @"C:\A", true)]
    [InlineData(@"C:\AB", @"C:\A", false)]
    [InlineData(@"C:\A", @"C:\A\B", false)]
    public void IsUnder(string path, string root, bool expected) =>
        Assert.Equal(expected, FolderRules.IsUnder(path, root));
}
