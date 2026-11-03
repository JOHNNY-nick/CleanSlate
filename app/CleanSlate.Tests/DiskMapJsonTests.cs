using System.Text.Json;
using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Decoding of cs_disk_map output and encoding of its request (D73).</summary>
public sealed class DiskMapJsonTests
{
    private const string Envelope = """
        {"ok":true,"data":{
          "root":{"name":"C:\\","path":"C:\\","size_bytes":300,"files":3,"category":"other","is_group":false,"is_folder":true,"children":[
            {"name":"OldTool","path":"D:\\OldTool","size_bytes":200,"files":2,"category":"leftovers","is_group":false,"is_folder":true,"children":[]},
            {"name":"Autres petits éléments","path":"","size_bytes":100,"files":1,"category":"user_files","is_group":true,"is_folder":false,"children":[]}]},
          "total_bytes":1000,"free_bytes":700,"denied":4,"duration_ms":12,"cancelled":false}}
        """;

    [Fact]
    public void DiskMapEnvelope_IsDecoded()
    {
        var response = JsonSerializer.Deserialize(Envelope, EngineJsonContext.Default.EngineResponseDiskMap);
        DiskMap map = EngineClient.Unwrap(response);

        Assert.Equal(1000UL, map.TotalBytes);
        Assert.Equal(700UL, map.FreeBytes);
        Assert.Equal(4UL, map.Denied);
        Assert.False(map.Cancelled);
        Assert.Equal(2, map.Root.Children.Count);
        Assert.Equal(StorageCategory.Leftovers, map.Root.Children[0].Category);
        Assert.True(map.Root.Children[1].IsGroup);
        Assert.Equal(StorageCategory.UserFiles, map.Root.Children[1].Category);
    }

    [Fact]
    public void DiskMapRequest_GoesThreeLevelsDown_ByDefault()
    {
        string json = JsonSerializer.Serialize(new DiskMapRequest(@"C:\"), EngineJsonContext.Default.DiskMapRequest);

        Assert.Equal("""{"root":"C:\\","max_depth":3}""", json);
    }
}
