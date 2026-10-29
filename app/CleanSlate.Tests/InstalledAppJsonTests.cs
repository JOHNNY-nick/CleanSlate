using System.Text.Json;
using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Decoding of cs_list_apps output (shape produced by serde in model.rs).</summary>
public sealed class InstalledAppJsonTests
{
    private const string Envelope = """
        {"ok":true,"data":[
          {"id":"dc0d1899da7b1454","display_name":"GIMP 2.10.38","publisher":"The GIMP Team",
           "version":"2.10.38","install_date":"2025-06-18","install_location":"C:\\Program Files\\GIMP 2",
           "estimated_size_kb":421888,"uninstall_string":"\"C:\\Program Files\\GIMP 2\\uninst\\unins000.exe\"",
           "quiet_uninstall_string":null,"is_msi":false,"product_code":null,"registry_view":"hklm64",
           "kind":"win32","hidden_reason":null,"installer":"inno_setup"},
          {"id":"0123456789abcdef","display_name":"VC Runtime","publisher":null,"version":null,
           "install_date":null,"install_location":null,"estimated_size_kb":null,"uninstall_string":null,
           "quiet_uninstall_string":null,"is_msi":true,"product_code":"{23170F69-40C1-2702-2408-000001000000}",
           "registry_view":"hklm32","kind":"win32","hidden_reason":"system_component","installer":"msi"}
        ]}
        """;

    [Fact]
    public void ListAppsEnvelope_IsDecoded()
    {
        var response = JsonSerializer.Deserialize(Envelope, EngineJsonContext.Default.EngineResponseListInstalledApp);
        List<InstalledApp> apps = EngineClient.Unwrap(response);

        Assert.Equal(2, apps.Count);
        InstalledApp gimp = apps[0];
        Assert.Equal("GIMP 2.10.38", gimp.DisplayName);
        Assert.Equal(new DateOnly(2025, 6, 18), gimp.InstallDate);
        Assert.Equal(@"C:\Program Files\GIMP 2", gimp.InstallLocation);
        Assert.Equal(421_888UL, gimp.EstimatedSizeKb);
        Assert.Equal(InstallerKind.InnoSetup, gimp.Installer);
        Assert.Equal(RegistryView.Hklm64, gimp.RegistryView);
        Assert.Null(gimp.HiddenReason);

        InstalledApp runtime = apps[1];
        Assert.Equal(HiddenReason.SystemComponent, runtime.HiddenReason);
        Assert.Equal(RegistryView.Hklm32, runtime.RegistryView);
        Assert.True(runtime.IsMsi);
        Assert.Equal(InstallerKind.Msi, runtime.Installer);
    }

    [Fact]
    public void Enums_SerializeInSnakeCase()
    {
        var options = new JsonSerializerOptions();
        Assert.Equal("\"inno_setup\"", JsonSerializer.Serialize(InstallerKind.InnoSetup, options));
        Assert.Equal("\"system_component\"", JsonSerializer.Serialize(HiddenReason.SystemComponent, options));
        Assert.Equal("\"hklm32\"", JsonSerializer.Serialize(RegistryView.Hklm32, options));
    }
}
