using CommunityToolkit.Mvvm.ComponentModel;

namespace CleanSlate.Presentation.Services;

/// <summary>
/// Long operations of the application (D82): the one running (analysis, duplicates, clean-up,
/// storage map, scan), its page and its progress, and what waits for the engine. The global
/// indicator of the window shows it on every page and leads back to it in one click; a page
/// left and opened again finds its operation here, still running or finished.
/// </summary>
public sealed partial class OperationService : ObservableObject
{
    private readonly List<Operation> _running = [];
    private int _waiting;

    /// <summary>An operation runs or waits for the engine: the indicator is shown.</summary>
    [ObservableProperty]
    private bool _isActive;

    /// <summary>« Analyse du disque · 120 000 fichiers · 80 Go ».</summary>
    [ObservableProperty]
    private string _text = string.Empty;

    /// <summary>Page of the operation shown (« Manager », « Storage »…), for the click.</summary>
    [ObservableProperty]
    private string? _pageTag;

    /// <summary>The operation shown can be cancelled from the indicator.</summary>
    [ObservableProperty]
    private bool _canCancel;

    /// <summary>
    /// Starts showing an operation; dispose the result when it ends. <paramref name="cancel"/>
    /// is what « Annuler » of the indicator calls.
    /// </summary>
    public Operation Begin(string title, string pageTag, Action? cancel = null)
    {
        var operation = new Operation(this, title, pageTag, cancel);
        _running.Add(operation);
        Refresh();
        return operation;
    }

    /// <summary>A request waits for the engine (D82: queued, never refused); dispose when it starts.</summary>
    public IDisposable Wait()
    {
        _waiting++;
        Refresh();
        return new Waiting(this);
    }

    /// <summary>Cancels the operation shown.</summary>
    public void Cancel() => _running.LastOrDefault()?.CancelAction?.Invoke();

    private void Refresh()
    {
        Operation? shown = _running.LastOrDefault();
        string waiting = _waiting == 0 ? string.Empty : _waiting == 1 ? " · 1 demande en attente" : $" · {_waiting} demandes en attente";
        IsActive = shown is not null || _waiting > 0;
        Text = shown is null
            ? _waiting > 0 ? "Une demande attend la fin de l'opération en cours" : string.Empty
            : (shown.Progress.Length > 0 ? $"{shown.Title} · {shown.Progress}" : shown.Title) + waiting;
        PageTag = shown?.PageTag;
        CanCancel = shown?.CancelAction is not null;
    }

    /// <summary>One running operation: report its progress, dispose it at the end.</summary>
    public sealed class Operation : IDisposable
    {
        private readonly OperationService _owner;

        internal Operation(OperationService owner, string title, string pageTag, Action? cancel)
        {
            _owner = owner;
            Title = title;
            PageTag = pageTag;
            CancelAction = cancel;
        }

        internal string Title { get; }

        internal string PageTag { get; }

        internal Action? CancelAction { get; }

        internal string Progress { get; private set; } = string.Empty;

        /// <summary>« 120 000 fichiers · 80 Go ».</summary>
        public void Report(string progress)
        {
            Progress = progress;
            _owner.Refresh();
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            _owner._running.Remove(this);
            _owner.Refresh();
        }
    }

    private sealed class Waiting(OperationService owner) : IDisposable
    {
        private bool _done;

        public void Dispose()
        {
            if (!_done)
            {
                _done = true;
                owner._waiting--;
                owner.Refresh();
            }
        }
    }
}
