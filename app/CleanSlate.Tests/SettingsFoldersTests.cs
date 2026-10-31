using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// Réglages, « Emplacements » (D63): the export folder changes at any time; the quarantine folder
/// only while the quarantine is empty, so that no session is left behind in the old folder.
/// </summary>
public sealed class SettingsFoldersTests
{
    private const string DefaultQuarantine = @"C:\Users\Ana\AppData\Local\CleanSlate";
    private const string DefaultExport = @"C:\Users\Ana\Desktop";

    private static Session SessionWith(SessionStatus status) =>
        new(Guid.NewGuid(), DateTimeOffset.Now, Origin.ForApp("g1"), [], 0, [], status, false);

    private static (SettingsViewModel Vm, ISettingsService Settings, IFolderService Folders) Create(params Session[] sessions)
    {
        var settings = Substitute.For<ISettingsService>();
        settings.QuarantineDays.Returns(30);
        settings.Exclusions.Returns([]);
        var engine = Substitute.For<IEngineService>();
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns(sessions);
        var folders = Substitute.For<IFolderService>();
        folders.DefaultQuarantineFolder.Returns(DefaultQuarantine);
        folders.DefaultExportFolder.Returns(DefaultExport);
        folders.CheckQuarantineFolder(Arg.Any<string>()).Returns(FolderProblem.None);
        folders.CheckExportFolder(Arg.Any<string>()).Returns(FolderProblem.None);
        return (new SettingsViewModel(settings, engine, folders: folders), settings, folders);
    }

    [Fact]
    public void ShowsTheDefaultFolders_WhenNoneWasChosen()
    {
        var (vm, _, _) = Create();
        Assert.Equal(DefaultQuarantine, vm.QuarantineFolder);
        Assert.Equal(DefaultExport, vm.ExportFolder);
    }

    [Fact]
    public async Task TheExportFolder_ChangesAtAnyTime_ButNeverToARefusedFolder()
    {
        var (vm, settings, folders) = Create(SessionWith(SessionStatus.Completed));
        folders.PickFolderAsync().Returns(@"C:\Windows\X", @"E:\Journaux");
        folders.CheckExportFolder(@"C:\Windows\X").Returns(FolderProblem.SystemFolder);

        await vm.ChangeExportFolderCommand.ExecuteAsync(null);
        Assert.Equal(FolderProblem.SystemFolder, vm.FolderProblem);
        settings.DidNotReceive().ExportFolder = Arg.Any<string?>();

        await vm.ChangeExportFolderCommand.ExecuteAsync(null);
        Assert.Equal(FolderProblem.None, vm.FolderProblem);
        settings.Received(1).ExportFolder = @"E:\Journaux";
        Assert.Equal(@"E:\Journaux", vm.ExportFolder);

        vm.UseDefaultExportFolderCommand.Execute(null);
        settings.Received(1).ExportFolder = null;
        Assert.Equal(DefaultExport, vm.ExportFolder);
    }

    [Theory]
    [InlineData(SessionStatus.Completed)]
    [InlineData(SessionStatus.Partial)]
    public async Task TheQuarantineFolder_StaysWhileItHoldsItems(SessionStatus status)
    {
        var (vm, _, _) = Create(SessionWith(SessionStatus.Restored), SessionWith(status));
        await vm.LoadCommand.ExecuteAsync(null);
        Assert.False(vm.CanChangeQuarantineFolder);
        Assert.False(vm.ChangeQuarantineFolderCommand.CanExecute(null));
    }

    [Fact]
    public async Task TheQuarantineFolder_MovesWhenEmpty_AfterItsCreation()
    {
        var (vm, settings, folders) = Create(SessionWith(SessionStatus.Restored), SessionWith(SessionStatus.Purged));
        folders.PickFolderAsync().Returns(@"D:\CleanSlate");
        await vm.LoadCommand.ExecuteAsync(null);
        Assert.True(vm.CanChangeQuarantineFolder);

        await vm.ChangeQuarantineFolderCommand.ExecuteAsync(null);

        Received.InOrder(() =>
        {
            folders.PrepareQuarantineFolder(@"D:\CleanSlate");
            settings.QuarantineFolder = @"D:\CleanSlate";
        });
        Assert.Equal(@"D:\CleanSlate", vm.QuarantineFolder);
    }
}
