using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using NSubstitute.ExceptionExtensions;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>AppsViewModel with a substituted IEngineService (EF-01 to EF-03).</summary>
public sealed class AppsViewModelTests
{
    private static InstalledApp App(
        string name,
        string? publisher = null,
        ulong? sizeKb = null,
        DateOnly? date = null,
        HiddenReason? hidden = null) =>
        new(
            Id: name,
            DisplayName: name,
            Publisher: publisher,
            Version: "1.0",
            InstallDate: date,
            InstallLocation: null,
            EstimatedSizeKb: sizeKb,
            UninstallString: null,
            QuietUninstallString: null,
            IsMsi: false,
            ProductCode: null,
            RegistryView: RegistryView.Hklm64,
            Kind: AppKind.Win32,
            HiddenReason: hidden,
            Installer: InstallerKind.Unknown);

    private static async Task<AppsViewModel> LoadedAsync(params InstalledApp[] apps)
    {
        var engine = Substitute.For<IEngineService>();
        engine.ListAppsAsync(Arg.Any<CancellationToken>()).Returns(apps);
        var viewModel = new AppsViewModel(engine, Substitute.For<INavigationService>());
        await viewModel.LoadCommand.ExecuteAsync(null);
        return viewModel;
    }

    private static string[] Names(AppsViewModel vm) => vm.Apps.Select(a => a.DisplayName).ToArray();

    [Fact]
    public async Task Load_SortsByNameAndHidesSystemComponents()
    {
        var vm = await LoadedAsync(
            App("VLC media player"),
            App("7-Zip"),
            App("VC++ Runtime", hidden: HiddenReason.SystemComponent),
            App("Update for X", hidden: HiddenReason.Update),
            App("audacity"));

        Assert.Equal(["7-Zip", "audacity", "VLC media player"], Names(vm));
        Assert.Equal(3, vm.VisibleCount);
        Assert.False(vm.HasError);
    }

    [Fact]
    public async Task ShowSystemComponents_RevealsHiddenEntries()
    {
        var vm = await LoadedAsync(App("A"), App("Runtime", hidden: HiddenReason.SystemComponent));

        vm.ShowSystemComponents = true;

        Assert.Equal(["A", "Runtime"], Names(vm));
    }

    [Fact]
    public async Task Search_MatchesNameOrPublisher_IgnoringCaseAndAccents()
    {
        var vm = await LoadedAsync(
            App("GIMP", "The GIMP Team"),
            App("Paint", "Éditions Créatives"),
            App("VLC", "VideoLAN"));

        vm.SearchText = "gim";
        Assert.Equal(["GIMP"], Names(vm));

        vm.SearchText = "editions creatives";
        Assert.Equal(["Paint"], Names(vm));

        vm.SearchText = "  ";
        Assert.Equal(3, vm.VisibleCount);
    }

    [Fact]
    public async Task Sort_BySizeAndDate_PutsUnknownLast()
    {
        var vm = await LoadedAsync(
            App("Small", sizeKb: 10, date: new DateOnly(2024, 1, 1)),
            App("Unknown"),
            App("Big", sizeKb: 1_000_000, date: new DateOnly(2026, 10, 1)),
            App("Medium", sizeKb: 5_000, date: new DateOnly(2025, 5, 5)));

        vm.SortIndex = (int)AppSortOrder.Size;
        Assert.Equal(["Big", "Medium", "Small", "Unknown"], Names(vm));

        vm.SortIndex = (int)AppSortOrder.InstallDate;
        Assert.Equal(["Big", "Medium", "Small", "Unknown"], Names(vm));

        vm.SortIndex = (int)AppSortOrder.Name;
        Assert.Equal(["Big", "Medium", "Small", "Unknown"], Names(vm));
    }

    [Fact]
    public async Task Selection_IsKeptWhileVisibleAndClearedOtherwise()
    {
        var vm = await LoadedAsync(App("GIMP"), App("VLC"));
        vm.SelectedApp = vm.Apps[0];
        Assert.True(vm.HasSelection);

        vm.SortIndex = (int)AppSortOrder.Size;
        Assert.Equal("GIMP", vm.SelectedApp?.DisplayName);

        vm.SearchText = "vlc";
        Assert.Null(vm.SelectedApp);
        Assert.False(vm.HasSelection);
    }

    [Fact]
    public async Task UninstallCommands_NeedAnUninstallerAndOpenTheWizard()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ListAppsAsync(Arg.Any<CancellationToken>()).Returns([App("GIMP") with { UninstallString = "u.exe" }, App("NoUninstaller")]);
        var navigation = Substitute.For<INavigationService>();
        var vm = new AppsViewModel(engine, navigation);
        await vm.LoadCommand.ExecuteAsync(null);

        Assert.False(vm.UninstallCompletelyCommand.CanExecute(null));
        vm.SelectedApp = vm.Apps[1];
        Assert.False(vm.UninstallCompletelyCommand.CanExecute(null));
        vm.SelectedApp = vm.Apps[0];
        Assert.True(vm.UninstallCompletelyCommand.CanExecute(null));
        Assert.True(vm.ClassicUninstallCommand.CanExecute(null));

        vm.UninstallCompletelyCommand.Execute(null);
        navigation.Received(1).OpenUninstallWizard(Arg.Is<InstalledApp>(a => a.DisplayName == "GIMP"), true);
        vm.ClassicUninstallCommand.Execute(null);
        navigation.Received(1).OpenUninstallWizard(Arg.Any<InstalledApp>(), false);
    }

    [Fact]
    public async Task Load_Failure_ShowsError()
    {
        var engine = Substitute.For<IEngineService>();
        engine.ListAppsAsync(Arg.Any<CancellationToken>()).ThrowsAsync(new EngineException("ACCESS_DENIED", "denied"));
        var vm = new AppsViewModel(engine, Substitute.For<INavigationService>());

        await vm.LoadCommand.ExecuteAsync(null);

        Assert.True(vm.HasError);
        Assert.Contains("ACCESS_DENIED", vm.ErrorDetails);
        Assert.Empty(vm.Apps);
        Assert.False(vm.IsLoading);
    }

    [Fact]
    public void Row_TextsAndInitials()
    {
        var row = new AppItemViewModel(App("GIMP 2.10.38", "The GIMP Team", 421_888, new DateOnly(2025, 6, 18)));

        Assert.Equal("412 Mo", row.SizeText);
        Assert.Equal("18 juin 2025", row.DateText);
        Assert.Equal("Gi", row.Initials);
        Assert.Equal("GIMP 2.10.38, The GIMP Team, 1.0, 412 Mo", row.AutomationName);
        Assert.Equal("No", new AppItemViewModel(App("Notepad++")).Initials);
        Assert.Equal("7z", new AppItemViewModel(App("7-Zip")).Initials);
        Assert.Equal("?", new AppItemViewModel(App("--")).Initials);
        Assert.Equal("—", new AppItemViewModel(App("X")).SizeText);
    }

    [Theory]
    [InlineData(512UL, "512 octets")]
    [InlineData(5_872_026UL, "5,6 Mo")]
    [InlineData(432_013_312UL, "412 Mo")]
    [InlineData(1_181_116_006UL, "1,1 Go")]
    [InlineData(10_485_760UL, "10 Mo")]
    public void Formats_Size(ulong bytes, string expected) => Assert.Equal(expected, Formats.Size(bytes));

    [Theory]
    [InlineData(2026, 10, 22, "22 oct. 2026")]
    [InlineData(2026, 2, 2, "2 févr. 2026")]
    [InlineData(2025, 3, 12, "12 mars 2025")]
    public void Formats_Date(int y, int m, int d, string expected) =>
        Assert.Equal(expected, Formats.Date(new DateOnly(y, m, d)));
}
