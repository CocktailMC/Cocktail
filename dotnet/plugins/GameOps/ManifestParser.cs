using System.Text;
using System.Text.Json;
using YamlDotNet.Serialization;

namespace Cocktail.Plugins.GameOps;

internal static class ManifestParser
{
    public static IReadOnlyList<StoredObject> Parse(string body)
    {
        var trimmed = body.Trim();
        if (trimmed.Length == 0)
        {
            return [];
        }

        if (trimmed.StartsWith('{') || trimmed.StartsWith('['))
        {
            return ParseJson(trimmed);
        }

        var docs = SplitYaml(trimmed);
        var deserializer = new DeserializerBuilder().IgnoreUnmatchedProperties().Build();
        var list = new List<StoredObject>();
        foreach (var doc in docs)
        {
            var raw = deserializer.Deserialize<object>(doc);
            if (raw is null)
            {
                continue;
            }

            var json = JsonSerializer.Serialize(Normalize(raw));
            list.AddRange(ParseJson(json));
        }

        return list;
    }

    private static List<StoredObject> ParseJson(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        if (root.ValueKind == JsonValueKind.Array)
        {
            return root.EnumerateArray().Select(FromElement).ToList();
        }

        return [FromElement(root)];
    }

    private static StoredObject FromElement(JsonElement el)
    {
        var kind = el.TryGetProperty("kind", out var k) ? k.GetString() ?? "" : "";
        var name = "";
        Dictionary<string, string> labels = new(StringComparer.OrdinalIgnoreCase);
        if (el.TryGetProperty("metadata", out var meta) && meta.ValueKind == JsonValueKind.Object)
        {
            name = meta.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "";
            if (meta.TryGetProperty("labels", out var labs) && labs.ValueKind == JsonValueKind.Object)
            {
                foreach (var p in labs.EnumerateObject())
                {
                    labels[p.Name] = p.Value.GetString() ?? p.Value.ToString();
                }
            }
        }

        if (string.IsNullOrWhiteSpace(name) && el.TryGetProperty("name", out var topName))
        {
            name = topName.GetString() ?? "";
        }

        var spec = el.TryGetProperty("spec", out var s) ? s.Clone() : JsonDocument.Parse("{}").RootElement.Clone();
        if (string.IsNullOrWhiteSpace(kind) || string.IsNullOrWhiteSpace(name))
        {
            throw new InvalidOperationException("manifest needs kind and metadata.name");
        }

        return new StoredObject
        {
            ApiVersion = el.TryGetProperty("apiVersion", out var av) ? av.GetString() ?? GameOpsKinds.ApiVersion : GameOpsKinds.ApiVersion,
            Kind = ObjectStore.NormalizeKind(kind),
            Name = name,
            Labels = labels,
            Spec = spec,
        };
    }

    private static IEnumerable<string> SplitYaml(string yaml)
    {
        var buf = new StringBuilder();
        using var reader = new StringReader(yaml);
        while (reader.ReadLine() is { } line)
        {
            if (line.Trim() == "---")
            {
                var chunk = buf.ToString().Trim();
                if (chunk.Length > 0)
                {
                    yield return chunk;
                }

                buf.Clear();
                continue;
            }

            buf.AppendLine(line);
        }

        var last = buf.ToString().Trim();
        if (last.Length > 0)
        {
            yield return last;
        }
    }

    private static object? Normalize(object? value) =>
        value switch
        {
            Dictionary<object, object> map => map.ToDictionary(
                kv => kv.Key.ToString() ?? "",
                kv => Normalize(kv.Value)),
            List<object> list => list.Select(Normalize).ToList(),
            _ => value,
        };
}
