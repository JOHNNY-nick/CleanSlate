using CleanSlate.Interop;
using System.Collections.ObjectModel;
using System.Globalization;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>Sort orders of the Applications list (EF-03).</summary>
public enum AppSortOrder
{
    /// <summary>Alphabetical.</summary>
    Name = 0,

    /// <summary>Largest first, unknown sizes last.</summary>
    Size = 1,

    /// <summary>Most recently installed first, unknown dates last.</summary>
    InstallDate = 2,
}

/// <summary>Installed applications: search, sort, system filter, selection (EF-01 to EF-03).</summary>
public sealed partial class AppsViewModel : ObservableObject
{
    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");
    private static readonly CompareInfo Compare = French.CompareInfo;
    private static readonly StringComparer NameComparer = StringComparer.Create(French, ignoreCase: true);
    private readonly IEngineService _engine;
    private readonly INavigationService _navigation;
    private List<AppItemViewModel> _all = [];

    /// <summary>Creates the ViewModel.</summary>
    public AppsViewModel(IEngineService engine, INavigationService navigation)
    {
        _engine = engine;
        _navigation = navigation;
    }

    /// <summary>Rows currently shown, filtered and sorted.</summary>
    public ObservableCollection<AppItemViewModel> Apps { get; } = [];

    /// <summary>Number of rows shown.</summary>
    public int VisibleCount => Apps.Count;

    /// <summary>Text typed in the search box (name or publisher).</summary>
    [ObservableProperty]
    private string _searchText = string.Empty;

    /// <summary>Selected sort order; bound to the « Trier par » combo box by index.</summary>
    [ObservableProperty]
    private int _sortIndex;

    /// <summary>Also shows system components and updates (EF-02).</summary>
    [ObservableProperty]
    private bool _showSystemComponents;

    /// <summary>Selected row, if any.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSelection))]
    [NotifyCanExecuteChangedFor(nameof(ClassicUninstallCommand), nameof(UninstallCompletelyCommand))]
    private AppItemViewModel? _selectedApp;

    /// <summary>True while the inventory is being read.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsEmpty))]
    private bool _isLoading;

    /// <summary>True when the inventory failed.</summary>
    [ObservableProperty]
    private bool _hasError;

    /// <summary>Technical details of the last error.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>True when a row is selected (shows the action bar).</summary>
    public bool HasSelection => SelectedApp is not null;

    /// <summary>True when loading succeeded but nothing matches (empty state).</summary>
    public bool IsEmpty => !IsLoading && !HasError && Apps.Count == 0;

    /// <summary>Current sort order.</summary>
    public AppSortOrder SortOrder => Enum.IsDefined(typeof(AppSortOrder), SortIndex) ? (AppSortOrder)SortIndex : AppSortOrder.Name;

    /// <summary>Reads the inventory.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        IsLoading = true;
        try
        {
            var apps = await _engine.ListAppsAsync(cancellationToken);
            _all = apps.Select(a => new AppItemViewModel(a)).ToList();
            HasError = false;
            ErrorDetails = string.Empty;
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            _all = [];
            HasError = true;
            ErrorDetails = ex.Message;
        }
        finally
        {
            IsLoading = false;
            Refresh();
        }
    }

    /// <summary>Official uninstaller only (EF-12 without EF-13).</summary>
    [RelayCommand(CanExecute = nameof(CanUninstall))]
    private void ClassicUninstall()
    {
        if (SelectedApp is { } row)
        {
            _navigation.OpenUninstallWizard(row.App, complete: false);
        }
    }

    /// <summary>Full uninstall with leftovers (EF-12, EF-13).</summary>
    [RelayCommand(CanExecute = nameof(CanUninstall))]
    private void UninstallCompletely()
    {
        if (SelectedApp is { } row)
        {
            _navigation.OpenUninstallWizard(row.App, complete: true);
        }
    }

    /// <summary>Win32 application with an uninstaller (Store apps are read-only, EF-04).</summary>
    private bool CanUninstall() =>
        SelectedApp?.App is { Kind: AppKind.Win32 } app && (app.UninstallString is not null || app.IsMsi);

    partial void OnSearchTextChanged(string value) => Refresh();

    partial void OnSortIndexChanged(int value) => Refresh();

    partial void OnShowSystemComponentsChanged(bool value) => Refresh();

    /// <summary>Rebuilds <see cref="Apps"/> from the full list, keeping the selection if still visible.</summary>
    private void Refresh()
    {
        string search = SearchText.Trim();
        IEnumerable<AppItemViewModel> rows = _all.Where(a => ShowSystemComponents || !a.IsHidden);
        if (search.Length > 0)
        {
            rows = rows.Where(a => Matches(a.DisplayName, search) || Matches(a.App.Publisher, search));
        }

        rows = SortOrder switch
        {
            AppSortOrder.Size => rows
                .OrderBy(a => a.App.EstimatedSizeKb is null)
                .ThenByDescending(a => a.App.EstimatedSizeKb)
                .ThenBy(a => a.DisplayName, NameComparer),
            AppSortOrder.InstallDate => rows
                .OrderBy(a => a.App.InstallDate is null)
                .ThenByDescending(a => a.App.InstallDate)
                .ThenBy(a => a.DisplayName, NameComparer),
            _ => rows.OrderBy(a => a.DisplayName, NameComparer),
        };

        AppItemViewModel? selected = SelectedApp;
        Apps.Clear();
        foreach (AppItemViewModel row in rows)
        {
            Apps.Add(row);
        }

        SelectedApp = selected is not null && Apps.Contains(selected) ? selected : null;
        OnPropertyChanged(nameof(VisibleCount));
        OnPropertyChanged(nameof(IsEmpty));
    }

    /// <summary>Case- and accent-insensitive « contains ».</summary>
    private static bool Matches(string? text, string search) =>
        text is not null && Compare.IndexOf(text, search, CompareOptions.IgnoreCase | CompareOptions.IgnoreNonSpace) >= 0;
}
