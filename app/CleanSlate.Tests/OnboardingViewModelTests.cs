using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using NSubstitute.ExceptionExtensions;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// First launch (D63): welcome with the logo, how CleanSlate works and acceptance, then the basic
/// settings (quarantine delay and folder, export folder), then the application.
/// </summary>
public sealed class OnboardingViewModelTests
{
    private const string DefaultQuarantine = @"C:\Users\Ana\AppData\Local\CleanSlate";
    private const string DefaultExport = @"C:\Users\Ana\Desktop";

    private static (OnboardingViewModel Vm, ISettingsService Settings, IFolderService Folders) Create()
    {
        var settings = Substitute.For<ISettingsService>();
        settings.QuarantineDays.Returns(30);
        var folders = Substitute.For<IFolderService>();
        folders.DefaultQuarantineFolder.Returns(DefaultQuarantine);
        folders.DefaultExportFolder.Returns(DefaultExport);
        folders.CheckQuarantineFolder(Arg.Any<string>()).Returns(FolderProblem.None);
        folders.CheckExportFolder(Arg.Any<string>()).Returns(FolderProblem.None);
        return (new OnboardingViewModel(settings, folders), settings, folders);
    }

    private static void GoToSettings(OnboardingViewModel vm)
    {
        vm.NextCommand.Execute(null);
        vm.Accepted = true;
        vm.NextCommand.Execute(null);
    }

    [Fact]
    public void StartsOnTheWelcome_WithTheDefaults()
    {
        var (vm, _, _) = Create();
        Assert.True(vm.IsStep1);
        Assert.Equal(30, vm.SelectedDays);
        Assert.Equal(DefaultQuarantine, vm.QuarantineFolder);
        Assert.Equal(DefaultExport, vm.ExportFolder);
        Assert.False(vm.BackCommand.CanExecute(null));
    }

    [Fact]
    public void TheSettings_AreReachedOnlyAfterAcceptance()
    {
        var (vm, _, _) = Create();
        vm.NextCommand.Execute(null);
        Assert.True(vm.IsStep2);
        Assert.False(vm.NextCommand.CanExecute(null));

        vm.Accepted = true;
        Assert.True(vm.NextCommand.CanExecute(null));
        vm.NextCommand.Execute(null);
        Assert.True(vm.IsStep3);

        vm.BackCommand.Execute(null);
        Assert.True(vm.IsStep2);
    }

    [Fact]
    public void Finish_WithTheDefaults_SavesTheDelayOnly_AndOpensTheApplication()
    {
        var (vm, settings, folders) = Create();
        bool completed = false;
        vm.Completed += (_, _) => completed = true;
        GoToSettings(vm);
        vm.SelectedDays = 7;

        vm.FinishCommand.Execute(null);

        folders.Received(1).PrepareQuarantineFolder(DefaultQuarantine);
        settings.Received(1).QuarantineDays = 7;
        settings.Received(1).QuarantineFolder = null;
        settings.Received(1).ExportFolder = null;
        settings.Received(1).FirstRunDone = true;
        Assert.True(completed);
    }

    [Fact]
    public async Task ChosenFolders_AreSaved()
    {
        var (vm, settings, folders) = Create();
        folders.PickFolderAsync().Returns(@"D:\CleanSlate", @"D:\Journaux");
        GoToSettings(vm);

        await vm.PickQuarantineFolderCommand.ExecuteAsync(null);
        await vm.PickExportFolderCommand.ExecuteAsync(null);
        vm.FinishCommand.Execute(null);

        folders.Received(1).PrepareQuarantineFolder(@"D:\CleanSlate");
        settings.Received(1).QuarantineFolder = @"D:\CleanSlate";
        settings.Received(1).ExportFolder = @"D:\Journaux";
    }

    [Fact]
    public void ARefusedFolder_IsExplained_AndBlocksTheEnd_UntilTheDefaultComesBack()
    {
        var (vm, settings, folders) = Create();
        folders.CheckQuarantineFolder(@"C:\Users\Ana\OneDrive\Q").Returns(FolderProblem.CloudSynced);
        GoToSettings(vm);

        vm.QuarantineFolder = @"C:\Users\Ana\OneDrive\Q";
        Assert.Equal(FolderProblem.CloudSynced, vm.QuarantineProblem);
        Assert.True(vm.HasQuarantineProblem);
        Assert.False(vm.FinishCommand.CanExecute(null));

        vm.UseDefaultQuarantineFolderCommand.Execute(null);
        Assert.Equal(DefaultQuarantine, vm.QuarantineFolder);
        Assert.False(vm.HasQuarantineProblem);
        Assert.True(vm.FinishCommand.CanExecute(null));
        settings.DidNotReceive().FirstRunDone = Arg.Any<bool>();
    }

    [Fact]
    public async Task APickerThatFails_AsksToTypeThePath()
    {
        var (vm, _, folders) = Create();
        folders.PickFolderAsync().ThrowsAsync(new InvalidOperationException("picker"));
        GoToSettings(vm);

        await vm.PickQuarantineFolderCommand.ExecuteAsync(null);

        Assert.True(vm.PickerFailed);
        Assert.Equal(DefaultQuarantine, vm.QuarantineFolder);
    }

    [Fact]
    public void AFolderThatCannotBeCreated_StopsTheEnd_WithItsReason()
    {
        var (vm, settings, folders) = Create();
        folders.When(f => f.PrepareQuarantineFolder(Arg.Any<string>())).Throw(new UnauthorizedAccessException("Accès refusé"));
        bool completed = false;
        vm.Completed += (_, _) => completed = true;
        GoToSettings(vm);

        vm.FinishCommand.Execute(null);

        Assert.False(completed);
        Assert.Equal("Accès refusé", vm.PrepareError);
        settings.DidNotReceive().FirstRunDone = Arg.Any<bool>();
    }
}
