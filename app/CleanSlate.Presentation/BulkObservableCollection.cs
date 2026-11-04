using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;

namespace CleanSlate.Presentation;

/// <summary>
/// A collection whose whole content is replaced with one notification (D84). The storage map
/// was cleared then filled rectangle by rectangle, and its page drew the whole map again at
/// each addition: 40 rectangles cost about 800 buttons per click.
/// </summary>
public sealed class BulkObservableCollection<T> : ObservableCollection<T>
{
    /// <summary>Replaces every item by <paramref name="items"/> and notifies once (Reset).</summary>
    public void ReplaceAll(IEnumerable<T> items)
    {
        Items.Clear();
        foreach (T item in items)
        {
            Items.Add(item);
        }

        OnPropertyChanged(new PropertyChangedEventArgs(nameof(Count)));
        OnPropertyChanged(new PropertyChangedEventArgs("Item[]"));
        OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
    }
}
