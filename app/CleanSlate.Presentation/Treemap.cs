using CleanSlate.Interop;

namespace CleanSlate.Presentation;

/// <summary>A rectangle of the treemap, in the coordinates of the drawing area.</summary>
/// <param name="X">Left.</param>
/// <param name="Y">Top.</param>
/// <param name="Width">Width.</param>
/// <param name="Height">Height.</param>
/// <param name="Index">Position of the item in the list given to <see cref="Treemap.Layout"/>.</param>
public readonly record struct TreemapRect(double X, double Y, double Width, double Height, int Index);

/// <summary>
/// Squarified treemap (Bruls, Huizing and van Wijk, 2000; the layout of WinDirStat and WizTree):
/// rectangles whose areas are proportional to the sizes, kept as square as possible so that
/// names fit (D73). Pure: the view only draws what it computes.
/// </summary>
public static class Treemap
{
    /// <summary>Lays out <paramref name="sizes"/> (any order) in the given area; empty sizes get no rectangle.</summary>
    public static IReadOnlyList<TreemapRect> Layout(IReadOnlyList<ulong> sizes, double width, double height)
    {
        var result = new List<TreemapRect>();
        double total = sizes.Aggregate(0.0, (s, v) => s + v);
        if (total <= 0 || width <= 0 || height <= 0)
        {
            return result;
        }

        // Areas in pixels, largest first.
        var items = sizes
            .Select((size, index) => (Area: size / total * width * height, Index: index))
            .Where(i => i.Area > 0)
            .OrderByDescending(i => i.Area)
            .ToList();
        double x = 0, y = 0, w = width, h = height;
        int start = 0;
        while (start < items.Count)
        {
            double side = Math.Min(w, h);
            int end = start + 1;
            double rowArea = items[start].Area;
            double best = Worst(items, start, end, rowArea, side);
            while (end < items.Count)
            {
                double candidate = Worst(items, start, end + 1, rowArea + items[end].Area, side);
                if (candidate > best)
                {
                    break;
                }

                best = candidate;
                rowArea += items[end].Area;
                end++;
            }

            // Lay the row along the shorter side, then shrink the free area.
            double thickness = rowArea / side;
            double offset = 0;
            for (int i = start; i < end; i++)
            {
                double length = items[i].Area / thickness;
                result.Add(w >= h
                    ? new TreemapRect(x, y + offset, thickness, length, items[i].Index)
                    : new TreemapRect(x + offset, y, length, thickness, items[i].Index));
                offset += length;
            }

            if (w >= h)
            {
                x += thickness;
                w -= thickness;
            }
            else
            {
                y += thickness;
                h -= thickness;
            }

            start = end;
        }

        return result;
    }

    // Worst aspect ratio of a row of items[start..end) whose areas sum to rowArea.
    private static double Worst(List<(double Area, int Index)> items, int start, int end, double rowArea, double side)
    {
        double max = items[start].Area;
        double min = items[end - 1].Area;
        double s2 = side * side;
        double r2 = rowArea * rowArea;
        return Math.Max(s2 * max / r2, r2 / (s2 * min));
    }
}

/// <summary>
/// Keeps a computed storage map up to date after CleanSlate acted (D73), without walking the
/// disk again: quarantined items leave their folders (they now weigh in the quarantine), a
/// purge or an immediate deletion makes the quarantine lighter.
/// </summary>
public static class DiskMapUpdate
{
    /// <summary>Items moved to <paramref name="quarantineRoot"/> by a session.</summary>
    public static DiskMap Moved(DiskMap map, IEnumerable<(string Path, ulong Size)> items, string quarantineRoot)
    {
        DiskMapNode root = map.Root;
        foreach ((string path, ulong size) in items)
        {
            // A map only knows its own drive: an item of another drive changes nothing here,
            // and the quarantine grows only if it is on this drive.
            if (Contains(root.Path, path))
            {
                root = Remove(root, path, size);
            }

            if (Contains(root.Path, quarantineRoot))
            {
                root = Add(root, quarantineRoot, size);
            }
        }

        return map with { Root = root };
    }

    /// <summary>Bytes deleted for good from <paramref name="quarantineRoot"/>: the space is free again.</summary>
    public static DiskMap Purged(DiskMap map, ulong bytes, string quarantineRoot) => Contains(map.Root.Path, quarantineRoot)
        ? map with { Root = Remove(map.Root, quarantineRoot, bytes, keepNode: true), FreeBytes = map.FreeBytes + bytes }
        : map;

    private static bool Contains(string node, string path) =>
        node.Length == 0 || string.Equals(node, path, StringComparison.OrdinalIgnoreCase)
        || path.StartsWith(node.TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase);

    // Subtracts size along the path; the node of the path itself disappears (unless kept).
    private static DiskMapNode Remove(DiskMapNode node, string path, ulong size, bool keepNode = false)
    {
        var children = new List<DiskMapNode>();
        foreach (DiskMapNode child in node.Children)
        {
            bool isTarget = string.Equals(child.Path, path, StringComparison.OrdinalIgnoreCase);
            if (!child.IsGroup && child.Path.Length > 0 && Contains(child.Path, path))
            {
                if (isTarget && !keepNode)
                {
                    continue;
                }

                DiskMapNode lighter = Remove(child, path, size, keepNode);
                if (lighter.SizeBytes > 0)
                {
                    children.Add(lighter);
                }
            }
            else
            {
                children.Add(child);
            }
        }

        return node with { SizeBytes = node.SizeBytes > size ? node.SizeBytes - size : 0, Children = children };
    }

    // Adds size along the path to the quarantine, as far as the map goes.
    private static DiskMapNode Add(DiskMapNode node, string path, ulong size)
    {
        var children = node.Children.Select(c => !c.IsGroup && c.Path.Length > 0 && Contains(c.Path, path) ? Add(c, path, size) : c).ToList();
        return node with { SizeBytes = node.SizeBytes + size, Children = children };
    }
}
