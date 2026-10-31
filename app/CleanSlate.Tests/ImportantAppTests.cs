using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// Important applications (D65): PowerShell 7, a runtime, a driver or an antivirus can be
/// uninstalled, but only after a warning and an explicit « I understand »; Windows components
/// and updates are never offered for uninstall.
/// </summary>
public sealed class ImportantAppTests
{
    private static InstalledApp App(string name, Caution? caution, HiddenReason? hidden = null) => new(
        "p1", name, "Microsoft Corporation", "7.5.0", null, @"C:\Program Files\PowerShell\7", 200_000,
        "MsiExec.exe /X{11111111-2222-3333-4444-555555555555}", null,
        true, "{11111111-2222-3333-4444-555555555555}", RegistryView.Hklm64, AppKind.Win32, hidden, InstallerKind.Msi,
        Caution: caution);

    private static UninstallWizardViewModel Wizard() => new(
        Substitute.For<IEngineService>(), OrphansViewModelTests.Dialogs(), OrphansViewModelTests.Settings(false), Substitute.For<INavigationService>());

    [Fact]
    public void AnImportantApplication_ShowsANotice_WithoutBlocking()
    {
        // The owner asked for a plain notice, not a box to tick (D67).
        var vm = Wizard();
        vm.Start(App("PowerShell 7-x64", Caution.Microsoft), complete: true);

        Assert.True(vm.HasCaution);
        Assert.Equal("Microsoft", vm.CautionKey);
        Assert.True(vm.LaunchCommand.CanExecute(null));
    }

    [Fact]
    public void AnOrdinaryApplication_HasNoWarning()
    {
        var vm = Wizard();
        vm.Start(App("VLC media player", null), complete: true);
        Assert.False(vm.HasCaution);
        Assert.True(vm.LaunchCommand.CanExecute(null));
    }

    [Theory]
    [InlineData(HiddenReason.SystemComponent)]
    [InlineData(HiddenReason.Update)]
    public void WindowsComponentsAndUpdates_AreNeverOfferedForUninstall(HiddenReason hidden)
    {
        var engine = Substitute.For<IEngineService>();
        var vm = new AppsViewModel(engine, Substitute.For<INavigationService>())
        {
            ShowSystemComponents = true,
        };
        vm.SelectedApp = new AppItemViewModel(App("Microsoft Visual C++ Additional Runtime", Caution.Runtime, hidden));
        Assert.False(vm.ClassicUninstallCommand.CanExecute(null));
        Assert.False(vm.UninstallCompletelyCommand.CanExecute(null));

        vm.SelectedApp = new AppItemViewModel(App("Microsoft Visual C++ 2015-2022 Redistributable", Caution.Runtime));
        Assert.True(vm.UninstallCompletelyCommand.CanExecute(null));
    }
}
