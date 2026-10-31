using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using NSubstitute;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// « Stockage » page (D69): free space of the disks before and after each action of CleanSlate,
/// space held in quarantine and space freed for good.
/// </summary>
public sealed class StorageTests
{
    private const ulong Gb = 1024UL * 1024 * 1024;
    private static readonly DateTimeOffset T0 = new(2026, 10, 31, 11, 0, 0, TimeSpan.FromHours(1));

    [Fact]
    public async Task EachAction_RecordsTheFreeSpaceBeforeAndAfter_OnEveryDiskConcerned()
    {
        var free = new Dictionary<string, ulong> { ["C:"] = 100 * Gb, ["D:"] = 400 * Gb };
        var recorded = new List<StorageEntry>();

        int result = await StorageMeasure.RunAsync(
            StorageOperation.Purge,
            _ => "30 jours",
            ["C:", "D:", "c:"],
            drive => free[drive.ToUpperInvariant()],
            () => T0,
            recorded.Add,
            async () =>
            {
                await Task.Yield();
                free["D:"] += 2 * Gb; // the quarantine was on D:
                return 42;
            },
            _ => 2 * Gb);

        Assert.Equal(42, result);
        Assert.Equal(2, recorded.Count);
        StorageEntry d = recorded.Single(e => e.Drive == "D:");
        Assert.Equal(400 * Gb, d.FreeBefore);
        Assert.Equal(402 * Gb, d.FreeAfter);
        Assert.Equal(2 * Gb, d.ReportedBytes);
        Assert.Equal(StorageOperation.Purge, d.Operation);
        Assert.Equal(T0, d.When);
    }

    [Fact]
    public async Task AFailedAction_IsNotRecorded()
    {
        var recorded = new List<StorageEntry>();
        await Assert.ThrowsAsync<InvalidOperationException>(() => StorageMeasure.RunAsync<int>(
            StorageOperation.Quarantine, _ => "", ["C:"], _ => 1, () => T0, recorded.Add,
            () => throw new InvalidOperationException("engine"), _ => 0));
        Assert.Empty(recorded);
    }

    [Fact]
    public async Task AnEmptyPurge_IsNotRecorded()
    {
        // The automatic purge runs at every start: when nothing was old enough, no line.
        var recorded = new List<StorageEntry>();
        await StorageMeasure.RunAsync(
            StorageOperation.Purge, _ => "", ["C:"], _ => 1, () => T0, recorded.Add,
            () => Task.FromResult(0UL), freed => freed);
        Assert.Empty(recorded);
    }

    [Fact]
    public void TheJournal_KeepsTheMostRecentEntries()
    {
        var journal = Enumerable.Range(0, 5)
            .Select(i => new StorageEntry(T0.AddMinutes(i), StorageOperation.Quarantine, "C:", 1, 1, 0, $"{i}"))
            .ToList();
        IReadOnlyList<StorageEntry> kept = StorageMeasure.Append(journal, new StorageEntry(T0.AddMinutes(9), StorageOperation.Purge, "C:", 1, 2, 1, "new"), max: 3);
        Assert.Equal(["3", "4", "new"], kept.Select(e => e.Detail));
    }

    [Fact]
    public async Task ThePage_ShowsDisks_QuarantineHeld_FreedForGood_AndEachAction()
    {
        var storage = Substitute.For<IStorageService>();
        storage.Drives().Returns([new DriveSpace("C:", 500 * Gb, 120 * Gb)]);
        storage.Journal().Returns(
        [
            new StorageEntry(T0, StorageOperation.Quarantine, "C:", 120 * Gb, 120 * Gb, 3 * Gb, "12 éléments"),
            new StorageEntry(T0.AddMinutes(5), StorageOperation.DeleteForGood, "C:", 120 * Gb, 121 * Gb, 1 * Gb, "1 session"),
            new StorageEntry(T0.AddMinutes(9), StorageOperation.Purge, "C:", 121 * Gb, 123 * Gb, 2 * Gb, "30 jours"),
        ]);
        var engine = Substitute.For<IEngineService>();
        var held = new QuarantinedItem("a", LeftoverKind.Folder, @"C:\A", @"C:\Q\a", 3 * Gb, null, null, T0, null, QuarantineStatus.Quarantined, null);
        var restored = held with { Id = "b", Status = QuarantineStatus.Restored };
        engine.ListSessionsAsync(Arg.Any<CancellationToken>()).Returns(
            [new Session(Guid.NewGuid(), T0, Origin.Orphans, [held, restored], 6 * Gb, [], SessionStatus.Partial, false)]);
        var vm = new StorageViewModel(storage, engine);

        await vm.LoadCommand.ExecuteAsync(null);

        DriveRow c = Assert.Single(vm.Drives);
        Assert.Equal("120 Go", c.FreeText);
        Assert.Equal("500 Go", c.TotalText);
        Assert.Equal(76, c.UsedPercent);
        Assert.Equal("3 Go", vm.QuarantineHeldText);
        Assert.Equal("3 Go", vm.FreedForGoodText); // purge + immediate deletion
        Assert.Equal(3, vm.Entries.Count);
        StorageEntryRow latest = vm.Entries[0]; // most recent first
        Assert.Equal(StorageOperation.Purge, latest.Operation);
        Assert.Equal("121 Go", latest.BeforeText);
        Assert.Equal("123 Go", latest.AfterText);
        Assert.Equal("+2 Go", latest.DifferenceText);
        Assert.Equal("0 octets", vm.Entries[2].DifferenceText);
    }
}
