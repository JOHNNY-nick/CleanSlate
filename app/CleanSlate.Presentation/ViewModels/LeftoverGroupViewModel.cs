using System.Collections.ObjectModel;
using CleanSlate.Interop;
using CommunityToolkit.Mvvm.ComponentModel;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>One proposed item: a checkbox, pre-checked only for High confidence (EF-10).</summary>
public sealed partial class LeftoverItemViewModel : ObservableObject
{
    /// <summary>Wraps a leftover.</summary>
    public LeftoverItemViewModel(Leftover leftover)
    {
        Leftover = leftover;
        _isChecked = leftover.Confidence == Confidence.High;
        SizeText = Formats.Size(leftover.SizeBytes);
        ReasonsText = string.Join(" · ", leftover.Reasons);
        AutomationName = $"{leftover.Location}, {SizeText}";
    }

    /// <summary>Engine record.</summary>
    public Leftover Leftover { get; }

    /// <summary>Kind (folder, key…), turned into text by the view.</summary>
    public LeftoverKind Kind => Leftover.Kind;

    /// <summary>Confidence, turned into a badge by the view.</summary>
    public Confidence Confidence => Leftover.Confidence;

    /// <summary>Full path or key.</summary>
    public string Location => Leftover.Location;

    /// <summary>Size, e.g. « 388 Mo ».</summary>
    public string SizeText { get; }

    /// <summary>Reasons of the proposal, joined.</summary>
    public string ReasonsText { get; }

    /// <summary>Text read by Narrator for the checkbox.</summary>
    public string AutomationName { get; }

    /// <summary>Selected for quarantine.</summary>
    [ObservableProperty]
    private bool _isChecked;

    /// <summary>Visible with the current confidence filter.</summary>
    [ObservableProperty]
    private bool _isVisible = true;

    /// <summary>
    /// Moved by a quarantine: hidden for good, but kept in its group's list, which is never
    /// changed once shown (WinUI crash of 31 Oct. 2026, D70).
    /// </summary>
    [ObservableProperty]
    private bool _isDone;
}

/// <summary>Leftovers of one guessed application (EF-09).</summary>
public sealed partial class LeftoverGroupViewModel : ObservableObject
{
    /// <summary>Wraps a group.</summary>
    public LeftoverGroupViewModel(LeftoverGroup group)
    {
        Group = group;
        Items = new ObservableCollection<LeftoverItemViewModel>(group.Items.Select(i => new LeftoverItemViewModel(i)));
        TotalText = Formats.Size(group.TotalBytes);
        _isExpanded = group.Items.Any(i => i.Confidence == Confidence.High);
    }

    /// <summary>Engine record.</summary>
    public LeftoverGroup Group { get; }

    /// <summary>Guessed application, e.g. « Audacity ».</summary>
    public string Label => Group.Label;

    /// <summary>Common publisher, or empty.</summary>
    public string PublisherText => Group.Publisher ?? string.Empty;

    /// <summary>True when the publisher is known.</summary>
    public bool HasPublisher => Group.Publisher is not null;

    /// <summary>« NVIDIA · » before the item count, or empty.</summary>
    public string PublisherPrefix => Group.Publisher is { } publisher ? $"{publisher} ·" : string.Empty;

    /// <summary>Number of items.</summary>
    public int ItemCount => Items.Count;

    /// <summary>Total size, e.g. « 412 Mo ».</summary>
    public string TotalText { get; }

    /// <summary>Items of the group.</summary>
    public ObservableCollection<LeftoverItemViewModel> Items { get; }

    /// <summary>Expanded in the list; groups with a High item start expanded.</summary>
    [ObservableProperty]
    private bool _isExpanded;

    /// <summary>Visible with the current confidence filter.</summary>
    [ObservableProperty]
    private bool _isVisible = true;
}
