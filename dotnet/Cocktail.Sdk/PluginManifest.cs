using System.Text.Json.Serialization;

namespace Cocktail.Sdk;

public sealed class PluginManifest
{
    [JsonPropertyName("id")]
    public required string Id { get; init; }

    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("version")]
    public string Version { get; init; } = "0.1.0";

    [JsonPropertyName("description")]
    public string Description { get; init; } = "";

    [JsonPropertyName("entryAssembly")]
    public string? EntryAssembly { get; init; }

    [JsonPropertyName("entryType")]
    public string? EntryType { get; init; }

    [JsonPropertyName("permissions")]
    public string[] Permissions { get; init; } = [];

    [JsonPropertyName("ui")]
    public PluginUi? Ui { get; init; }
}

public sealed class PluginUi
{
    [JsonPropertyName("label")]
    public string Label { get; init; } = "";

    [JsonPropertyName("icon")]
    public string Icon { get; init; } = "fa-puzzle-piece";

    [JsonPropertyName("path")]
    public string Path { get; init; } = "/summary";
}
