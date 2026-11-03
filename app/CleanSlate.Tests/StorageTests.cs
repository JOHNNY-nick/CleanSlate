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
    public void Difference_IsSigned()
    {
        Assert.Equal("+2 Go", StorageMeasure.Difference(121 * Gb, 123 * Gb));
        Assert.Equal("−1 Go", StorageMeasure.Difference(3 * Gb, 2 * Gb));
        Assert.Equal("0 octets", StorageMeasure.Difference(Gb, Gb));
    }
}
