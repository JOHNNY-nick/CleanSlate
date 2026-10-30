using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Log of the unexpected errors of the interface (D56).</summary>
public sealed class CrashLogTests
{
    private static readonly DateTimeOffset When = new(2026, 10, 30, 14, 1, 48, TimeSpan.FromHours(1));

    private static Exception Thrown()
    {
        try
        {
            try
            {
                throw new InvalidOperationException("inner cause");
            }
            catch (InvalidOperationException inner)
            {
                throw new ApplicationException("outer failure", inner);
            }
        }
        catch (ApplicationException e)
        {
            return e;
        }
    }

    [Fact]
    public void An_entry_holds_the_type_message_stack_and_inner_exception()
    {
        string entry = CrashLog.Format(Thrown(), "Application.UnhandledException", When, "XAML message");

        Assert.Contains("2026-10-30 14:01:48", entry);
        Assert.Contains("Application.UnhandledException", entry);
        Assert.Contains("XAML message", entry);
        Assert.Contains("System.ApplicationException: outer failure", entry);
        Assert.Contains("System.InvalidOperationException: inner cause", entry);
        Assert.Contains(nameof(Thrown), entry);
    }

    [Fact]
    public void Entries_are_appended_to_the_file_of_the_day()
    {
        string dir = Path.Combine(Path.GetTempPath(), "CleanSlateTests", Guid.NewGuid().ToString("N"));
        try
        {
            string? first = CrashLog.Write(dir, Thrown(), "one", When);
            string? second = CrashLog.Write(dir, Thrown(), "two", When);

            Assert.NotNull(first);
            Assert.Equal(first, second);
            Assert.Equal(Path.Combine(dir, "crash.20261030.log"), first);
            string text = File.ReadAllText(first!);
            Assert.Contains("— one", text);
            Assert.Contains("— two", text);
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void Writing_never_throws()
    {
        // The log folder cannot be created: a file already has its name.
        string file = Path.GetTempFileName();
        try
        {
            Assert.Null(CrashLog.Write(file, Thrown(), "source", When));
        }
        finally
        {
            File.Delete(file);
        }
    }

    [Theory]
    [InlineData(null, @"C:\Users\x\AppData\Local\CleanSlate\logs")]
    [InlineData("", @"C:\Users\x\AppData\Local\CleanSlate\logs")]
    [InlineData(@"relative\logs", @"C:\Users\x\AppData\Local\CleanSlate\logs")]
    [InlineData(@"C:\CleanSlate\results\complements-1\cleanslate-logs", @"C:\CleanSlate\results\complements-1\cleanslate-logs")]
    public void The_folder_can_only_be_moved_to_an_absolute_path(string? variable, string expected)
    {
        Assert.Equal(expected, CrashLog.LogDirectory(variable, @"C:\Users\x\AppData\Local"));
    }
}
