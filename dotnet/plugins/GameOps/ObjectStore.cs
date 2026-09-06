using System.Text.Json;
using Cocktail.Sdk;

namespace Cocktail.Plugins.GameOps;

internal sealed class ObjectStore
{
    private readonly string _root;
    private readonly object _gate = new();

    public ObjectStore(string dataDirectory)
    {
        _root = Path.Combine(dataDirectory, "objects");
        Directory.CreateDirectory(_root);
        foreach (var kind in GameOpsKinds.Owned)
        {
            Directory.CreateDirectory(Path.Combine(_root, kind.ToLowerInvariant()));
        }
    }

    public IReadOnlyList<StoredObject> List(string? kind = null)
    {
        lock (_gate)
        {
            var kinds = string.IsNullOrEmpty(kind)
                ? GameOpsKinds.Owned
                : [NormalizeKind(kind)];
            var list = new List<StoredObject>();
            foreach (var k in kinds)
            {
                var dir = Path.Combine(_root, k.ToLowerInvariant());
                if (!Directory.Exists(dir))
                {
                    continue;
                }

                foreach (var file in Directory.GetFiles(dir, "*.json"))
                {
                    var obj = JsonSerializer.Deserialize<StoredObject>(File.ReadAllText(file), PluginJson.Options);
                    if (obj is not null)
                    {
                        list.Add(obj);
                    }
                }
            }

            return list.OrderBy(o => o.Kind).ThenBy(o => o.Name).ToList();
        }
    }

    public StoredObject? Get(string kind, string name)
    {
        lock (_gate)
        {
            var path = FilePath(kind, name);
            if (!File.Exists(path))
            {
                return null;
            }

            return JsonSerializer.Deserialize<StoredObject>(File.ReadAllText(path), PluginJson.Options);
        }
    }

    public StoredObject Upsert(string kind, string name, JsonElement spec, Dictionary<string, string>? labels = null)
    {
        kind = NormalizeKind(kind);
        if (!GameOpsKinds.Owned.Contains(kind, StringComparer.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"kind {kind} is observed-only; not stored in GameOps");
        }

        lock (_gate)
        {
            var existing = GetUnlocked(kind, name);
            var obj = new StoredObject
            {
                Kind = kind,
                Name = name,
                Spec = spec,
                Labels = labels ?? existing?.Labels ?? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase),
                Status = existing?.Status ?? SpecJson.Write(new { message = "pending" }),
                Generation = (existing?.Generation ?? 0) + 1,
                UpdatedAt = DateTimeOffset.UtcNow,
            };
            File.WriteAllText(FilePath(kind, name), JsonSerializer.Serialize(obj, PluginJson.Options));
            return obj;
        }
    }

    public void PutStatus(string kind, string name, object status)
    {
        lock (_gate)
        {
            var obj = GetUnlocked(kind, name);
            if (obj is null)
            {
                return;
            }

            obj.Status = SpecJson.Write(status);
            obj.UpdatedAt = DateTimeOffset.UtcNow;
            File.WriteAllText(FilePath(kind, name), JsonSerializer.Serialize(obj, PluginJson.Options));
        }
    }

    public bool Delete(string kind, string name)
    {
        lock (_gate)
        {
            var path = FilePath(kind, name);
            if (!File.Exists(path))
            {
                return false;
            }

            File.Delete(path);
            return true;
        }
    }

    private StoredObject? GetUnlocked(string kind, string name)
    {
        var path = FilePath(kind, name);
        return File.Exists(path)
            ? JsonSerializer.Deserialize<StoredObject>(File.ReadAllText(path), PluginJson.Options)
            : null;
    }

    private string FilePath(string kind, string name)
    {
        var safe = string.Join("_", name.Split(Path.GetInvalidFileNameChars(), StringSplitOptions.RemoveEmptyEntries));
        return Path.Combine(_root, NormalizeKind(kind).ToLowerInvariant(), safe + ".json");
    }

    public static string NormalizeKind(string kind) =>
        GameOpsKinds.Owned.FirstOrDefault(k => k.Equals(kind, StringComparison.OrdinalIgnoreCase))
        ?? kind;
}
