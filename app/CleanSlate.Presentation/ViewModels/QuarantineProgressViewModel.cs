using CleanSlate.Interop;
using CommunityToolkit.Mvvm.ComponentModel;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>
/// Progress bar of a quarantine (task 2 of the owner): the share of the bytes moved, « 3 / 12
/// élément(s) · 1,2 Go sur 3 Go » and the item being moved. Shown while the engine works.
/// </summary>
public sealed partial class QuarantineProgressViewModel : ObservableObject
{
    /// <summary>A quarantine is running.</summary>
    [ObservableProperty]
    private bool _isActive;

    /// <summary>0 to 100.</summary>
    [ObservableProperty]
    private double _percent;

    /// <summary>« 3 / 12 élément(s) · 1,2 Go sur 3 Go ».</summary>
    [ObservableProperty]
    private string _text = string.Empty;

    /// <summary>Folder, file or key being moved.</summary>
    [ObservableProperty]
    private string _current = string.Empty;

    /// <summary>Shows the bar and returns the progress to give to the engine.</summary>
    public IProgress<CleanProgress> Start()
    {
        Percent = 0;
        Text = string.Empty;
        Current = string.Empty;
        IsActive = true;
        return new Progress<CleanProgress>(Report);
    }

    /// <summary>One event of the engine.</summary>
    public void Report(CleanProgress p)
    {
        Percent = p.BytesTotal > 0 ? 100.0 * p.BytesDone / p.BytesTotal : p.Total > 0 ? 100.0 * p.Done / p.Total : 100;
        Text = $"{p.Done} / {p.Total} élément(s) · {Formats.Size(p.BytesDone)} sur {Formats.Size(p.BytesTotal)}";
        Current = p.Current;
    }

    /// <summary>Hides the bar.</summary>
    public void Stop() => IsActive = false;
}
