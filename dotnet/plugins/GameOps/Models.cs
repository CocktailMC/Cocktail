using System.Text.Json;
using System.Text.Json.Serialization;

namespace Cocktail.Plugins.GameOps;

public static class GameOpsKinds
{
    public const string ApiVersion = "cocktail.gameops/v1";
    public const string World = "World";
    public const string PluginSet = "PluginSet";
    public const string Proxy = "Proxy";
    public const string Network = "Network";

    public static readonly string[] Owned = [World, PluginSet, Proxy, Network];
    public static readonly string[] Observed = ["Node", "Instance"];
}

public sealed class StoredObject
{
    public string ApiVersion { get; set; } = GameOpsKinds.ApiVersion;
    public string Kind { get; set; } = "";
    public string Name { get; set; } = "";
    public Dictionary<string, string> Labels { get; set; } = new(StringComparer.OrdinalIgnoreCase);
    public JsonElement Spec { get; set; }
    public JsonElement Status { get; set; }
    public ulong Generation { get; set; } = 1;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}

public sealed class WorldSpec
{
    public string? Path { get; set; }
    public string? CloneFrom { get; set; }
    public string Folder { get; set; } = "world";
    public int RetainSnapshots { get; set; } = 5;
}

public sealed class PluginSetItem
{
    public string Name { get; set; } = "";
    public bool Enabled { get; set; } = true;
    public string Source { get; set; } = "local";
    public string? Project { get; set; }
    public string? Version { get; set; }
}

public sealed class PluginSetSpec
{
    public string? Group { get; set; }
    public List<string> InstanceNames { get; set; } = [];
    public List<PluginSetItem> Items { get; set; } = [];
}

public sealed class ProxySpec
{
    public string? InstanceName { get; set; }
    public string? NodeId { get; set; }
    public int ListenPort { get; set; } = 25577;
    public string? Group { get; set; }
    public string Motd { get; set; } = "Cocktail GameOps";
    public bool CreateIfMissing { get; set; } = true;
    public bool DesiredRunning { get; set; } = true;
}

public sealed class NetworkServer
{
    public string Name { get; set; } = "";
    public string? NodeId { get; set; }
    public string Core { get; set; } = "paper";
    public int MemoryMib { get; set; } = 2048;
    public int Port { get; set; } = 25565;
    public string? World { get; set; }
    public string Group { get; set; } = "default";
}

public sealed class NetworkSpec
{
    public List<string> Worlds { get; set; } = [];
    public string? PluginSet { get; set; }
    public List<NetworkServer> Servers { get; set; } = [];
    public string? Proxy { get; set; }
    public bool DesiredRunning { get; set; } = true;
}

public static class SpecJson
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static T Read<T>(JsonElement spec) where T : new()
    {
        if (spec.ValueKind is JsonValueKind.Undefined or JsonValueKind.Null)
        {
            return new T();
        }

        return spec.Deserialize<T>(Options) ?? new T();
    }

    public static JsonElement Write(object value) =>
        JsonSerializer.SerializeToElement(value, Options);
}
