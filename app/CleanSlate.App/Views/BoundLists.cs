using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CleanSlate.App.Views;

/// <summary>
/// The lists of a page whose ViewModel outlives it (D82: Gestionnaire, Stockage). A list keeps
/// listening to the collection it shows: once its page is gone, a change of the collection
/// reached it anyway and WinUI threw (crash of the capture of 4 Nov. 2026, COMException
/// 0x80004005 in StorageMapViewModel.Layout). The page releases its lists when it is left.
/// </summary>
internal static class BoundLists
{
    /// <summary>Detaches every list below <paramref name="root"/> from its collection.</summary>
    public static void Release(DependencyObject root)
    {
        foreach (DependencyObject element in PaintDecorator.Descendants(root).ToList())
        {
            switch (element)
            {
                case ItemsControl list:
                    list.ItemsSource = null;
                    break;
                case ItemsRepeater repeater:
                    repeater.ItemsSource = null;
                    break;
            }
        }
    }
}
