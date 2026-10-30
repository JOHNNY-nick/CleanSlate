using System.Runtime.InteropServices;
using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.ViewModels;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// The text of an enum value shown by the views. Crash of 30 Oct. 2026 in Windows Sandbox:
/// selecting VLC on the Applications page closed CleanSlate without a message, because the
/// view asked for « HiddenReason_ » (empty reason) and the resource loader of the Windows App
/// SDK throws COMException 0x80073B17 for a missing key (checked against CleanSlate.App.pri).
/// </summary>
public sealed class ResourceTextTests
{
    private const int NamedResourceNotFound = unchecked((int)0x80073B17);

    private static readonly Dictionary<string, string> Strings = new()
    {
        ["HiddenReason_MicrosoftApp"] = "Application Microsoft",
        ["HiddenReason_Preinstalled"] = "Préinstallée",
        ["Kind_Folder"] = "Dossier",
    };

    /// <summary>Behaves like MRT Core's ResourceLoader.GetString: throws for a missing key.</summary>
    private static string MrtGetString(string key) =>
        Strings.TryGetValue(key, out string? text) ? text : throw new COMException("NamedResource introuvable.", NamedResourceNotFound);

    // VLC as the MSI installer writes it: no install folder, size or date.
    private static InstalledApp Win32App(HiddenReason? reason = null) => new(
        Id: "hklm64:{585C109D-9729-4B98-A2B8-EACD8885D115}",
        DisplayName: "VLC media player",
        Publisher: "VideoLAN",
        Version: "3.0.24",
        InstallDate: null,
        InstallLocation: null,
        EstimatedSizeKb: null,
        UninstallString: "MsiExec.exe /X{585C109D-9729-4B98-A2B8-EACD8885D115}",
        QuietUninstallString: null,
        IsMsi: true,
        ProductCode: "{585C109D-9729-4B98-A2B8-EACD8885D115}",
        RegistryView: RegistryView.Hklm64,
        Kind: AppKind.Win32,
        HiddenReason: reason,
        Installer: InstallerKind.Msi);

    [Fact]
    public void Selecting_an_application_without_hidden_reason_never_throws()
    {
        // The exact path of the crash: the view converts SelectedApp.HiddenReasonKey.
        var vlc = new AppItemViewModel(Win32App());

        string text = ResourceText.Resolve(MrtGetString, "HiddenReason", vlc.HiddenReasonKey);

        Assert.Equal(string.Empty, text);
    }

    [Theory]
    [InlineData(HiddenReason.SystemComponent)]
    [InlineData(HiddenReason.Update)]
    public void A_value_without_text_falls_back_to_its_name(HiddenReason reason)
    {
        var app = new AppItemViewModel(Win32App(reason));

        Assert.Equal(reason.ToString(), ResourceText.Resolve(MrtGetString, "HiddenReason", app.HiddenReasonKey));
    }

    [Fact]
    public void A_known_value_gets_its_french_text()
    {
        Assert.Equal("Préinstallée", ResourceText.Resolve(MrtGetString, "HiddenReason", HiddenReason.Preinstalled));
        Assert.Equal("Dossier", ResourceText.Resolve(MrtGetString, "Kind", "Folder"));
    }

    [Fact]
    public void A_null_value_is_never_looked_up()
    {
        bool called = false;

        string text = ResourceText.Resolve(key => { called = true; return MrtGetString(key); }, "Kind", null);

        Assert.Equal(string.Empty, text);
        Assert.False(called);
    }
}
