using System.IO.Compression;
using System.Text;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using NSubstitute.ExceptionExtensions;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>« Exporter les journaux » (Réglages): zip of the logs and settings, personal parts replaced.</summary>
public sealed class LogExportTests : IDisposable
{
    private const string Profile = @"C:\Users\Ana Martin";
    private static readonly IReadOnlyList<KeyValuePair<string, string>> Map = LogExport.Replacements(Profile, "Ana Martin", "PC-SALON");

    private readonly string _work = Path.Combine(Path.GetTempPath(), "cleanslate-export-" + Guid.NewGuid().ToString("N"));

    public LogExportTests() => Directory.CreateDirectory(_work);

    public void Dispose() => Directory.Delete(_work, recursive: true);

    [Theory]
    [InlineData(@"scan C:\Users\Ana Martin\AppData\Local\GIMP", @"scan %USERPROFILE%\AppData\Local\GIMP")]
    [InlineData(@"c:\users\ana martin\Desktop", @"%USERPROFILE%\Desktop")]
    [InlineData(@"{""path"":""C:\\Users\\Ana Martin\\AppData""}", @"{""path"":""%USERPROFILE%\\AppData""}")]
    [InlineData(@"owner PC-SALON\Ana Martin", @"owner <ordinateur>\<utilisateur>")]
    [InlineData("host PC-SALON, build 26100", "host <ordinateur>, build 26100")]
    [InlineData(@"C:\Users\Ana Martinez\x", @"C:\Users\Ana Martinez\x")]
    [InlineData("PC-SALON2 and MYPC-SALON stay", "PC-SALON2 and MYPC-SALON stay")]
    [InlineData(@"C:\Program Files\VideoLAN", @"C:\Program Files\VideoLAN")]
    public void Redact_ReplacesWholePersonalNamesOnly(string text, string expected) =>
        Assert.Equal(expected, LogExport.Redact(text, Map));

    [Fact]
    public void ShortAccountNames_AreNotReplacedAlone()
    {
        var map = LogExport.Replacements(@"C:\Users\HP", "HP", "DESKTOP-1");
        Assert.Equal(@"%USERPROFILE%\x ; PHP ; HP", LogExport.Redact(@"C:\Users\HP\x ; PHP ; HP", map));
    }

    [Fact]
    public void Zip_HoldsLogsSettingsAndSystem_WithoutThePersonalFolder()
    {
        string logs = Directory.CreateDirectory(Path.Combine(_work, "logs")).FullName;
        File.WriteAllText(Path.Combine(logs, "cleanslate.2026-10-30.log"), $@"quarantine {Profile}\AppData\Roaming\GIMP");
        File.WriteAllText(Path.Combine(logs, "crash.20261030.log"), "Process 12 on PC-SALON");
        File.WriteAllText(Path.Combine(logs, "notes.txt"), "not a log");
        string settings = Path.Combine(_work, "settings.json");
        File.WriteAllText(settings, @"{""SimulationMode"":true,""Exclusions"":[""C:\\Users\\Ana Martin\\Documents\\Keep""]}");
        string zipPath = Path.Combine(_work, "export.zip");

        // Today's log stays open by the engine while the export reads it.
        using (new FileStream(Path.Combine(logs, "cleanslate.2026-10-30.log"), FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite))
        {
            Assert.Equal(2, LogExport.Create(zipPath, logs, settings, "Windows 11 on PC-SALON", Map));
        }

        using var zip = ZipFile.OpenRead(zipPath);
        Assert.Equal(
            ["configuration/settings.json", "journaux/cleanslate.2026-10-30.log", "journaux/crash.20261030.log", "systeme.txt"],
            zip.Entries.Select(e => e.FullName).Order(StringComparer.Ordinal));
        var texts = zip.Entries.ToDictionary(e => e.FullName, e => new StreamReader(e.Open(), Encoding.UTF8).ReadToEnd());
        Assert.All(texts.Values, t => Assert.DoesNotContain("Ana Martin", t));
        Assert.All(texts.Values, t => Assert.DoesNotContain("PC-SALON", t));
        Assert.Equal(@"quarantine %USERPROFILE%\AppData\Roaming\GIMP", texts["journaux/cleanslate.2026-10-30.log"]);
        Assert.Contains(@"%USERPROFILE%\\Documents\\Keep", texts["configuration/settings.json"]);
        Assert.Equal("Windows 11 on <ordinateur>", texts["systeme.txt"]);
    }

    [Fact]
    public void Zip_WithoutLogsOrSettings_StillHoldsTheSystem_AndNeverOverwrites()
    {
        string zipPath = Path.Combine(_work, "export.zip");
        Assert.Equal(0, LogExport.Create(zipPath, Path.Combine(_work, "missing"), null, "system", Map));
        using (var zip = ZipFile.OpenRead(zipPath))
        {
            Assert.Equal(["systeme.txt"], zip.Entries.Select(e => e.FullName));
        }

        Assert.Throws<IOException>(() => LogExport.Create(zipPath, _work, null, "again", Map));
    }

    [Fact]
    public async Task Settings_ExportShowsThePath_OrTheError()
    {
        var settings = Substitute.For<ISettingsService>();
        var export = Substitute.For<ILogExportService>();
        export.ExportAsync(Arg.Any<CancellationToken>()).Returns(@"C:\Users\Ana Martin\Desktop\CleanSlate-journaux.zip");
        var vm = new SettingsViewModel(settings, logExport: export);

        await vm.ExportLogsCommand.ExecuteAsync(null);
        Assert.True(vm.HasExport);
        Assert.False(vm.ExportFailed);
        vm.RevealExportCommand.Execute(null);
        export.Received(1).Reveal(@"C:\Users\Ana Martin\Desktop\CleanSlate-journaux.zip");

        export.ExportAsync(Arg.Any<CancellationToken>()).ThrowsAsync(new UnauthorizedAccessException("Accès refusé"));
        await vm.ExportLogsCommand.ExecuteAsync(null);
        Assert.False(vm.HasExport);
        Assert.True(vm.ExportFailed);
        Assert.Equal("Accès refusé", vm.ExportError);
    }
}
