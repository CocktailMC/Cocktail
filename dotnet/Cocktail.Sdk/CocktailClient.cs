using System.Net.Http.Json;
using System.Text.Json.Serialization;

namespace Cocktail.Sdk;

public interface ICocktailClient
{
    Task<IReadOnlyList<JsonInstance>> ListInstancesAsync(CancellationToken ct = default);
    Task<JsonInstance?> GetInstanceAsync(string id, CancellationToken ct = default);
    Task<JsonInstance> StartInstanceAsync(string id, CancellationToken ct = default);
    Task<JsonInstance> StopInstanceAsync(string id, CancellationToken ct = default);
    Task<IReadOnlyList<JsonNode>> ListNodesAsync(CancellationToken ct = default);
    Task<JsonInstance> CreateInstanceAsync(CreateInstanceBody body, CancellationToken ct = default);
    Task<JsonInstance> ApplySpecAsync(string id, string yaml, CancellationToken ct = default);
    Task<IReadOnlyList<JsonPlugin>> ListPluginsAsync(string instanceId, CancellationToken ct = default);
    Task EnablePluginAsync(string instanceId, string name, bool enabled, CancellationToken ct = default);
    Task WriteFileAsync(string instanceId, string path, string content, CancellationToken ct = default);
    Task<IReadOnlyList<JsonFileEntry>> ListFilesAsync(string instanceId, string path = "", CancellationToken ct = default);
    Task<string> ReadFileAsync(string instanceId, string path, CancellationToken ct = default);
    Task MkdirAsync(string instanceId, string path, CancellationToken ct = default);
    Task UploadBytesAsync(string instanceId, string path, byte[] bytes, string fileName, CancellationToken ct = default);
    Task InstallModrinthAsync(string instanceId, string projectId, string? versionId = null, CancellationToken ct = default);
}

public sealed class JsonFileEntry
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("path")]
    public string Path { get; set; } = "";

    [JsonPropertyName("is_dir")]
    public bool IsDir { get; set; }

    [JsonPropertyName("size")]
    public long Size { get; set; }
}

public sealed class JsonNode
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = "";

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("kind")]
    public string Kind { get; set; } = "";

    [JsonPropertyName("online")]
    public bool Online { get; set; }

    [JsonPropertyName("hostname")]
    public string? Hostname { get; set; }
}

public sealed class JsonPlugin
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("path")]
    public string Path { get; set; } = "";

    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; }

    [JsonPropertyName("size")]
    public long Size { get; set; }
}

public sealed class CreateInstanceBody
{
    [JsonPropertyName("name")]
    public required string Name { get; set; }

    [JsonPropertyName("core")]
    public string? Core { get; set; }

    [JsonPropertyName("memory_mib")]
    public int? MemoryMib { get; set; }

    [JsonPropertyName("port")]
    public int? Port { get; set; }

    [JsonPropertyName("node_id")]
    public string? NodeId { get; set; }

    [JsonPropertyName("group")]
    public string? Group { get; set; }

    [JsonPropertyName("tags")]
    public List<string>? Tags { get; set; }

    [JsonPropertyName("eula_accepted")]
    public bool EulaAccepted { get; set; }

    [JsonPropertyName("auto_restart")]
    public bool AutoRestart { get; set; }
}

public sealed class JsonInstance
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = "";

    [JsonPropertyName("status")]
    public string Status { get; set; } = "";

    [JsonPropertyName("node_id")]
    public string? NodeId { get; set; }

    [JsonPropertyName("desired_running")]
    public bool DesiredRunning { get; set; }

    [JsonPropertyName("generation")]
    public ulong Generation { get; set; }

    [JsonPropertyName("spec")]
    public JsonInstanceSpec Spec { get; set; } = new();
}

public sealed class JsonInstanceSpec
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("workdir")]
    public string Workdir { get; set; } = "";

    [JsonPropertyName("core")]
    public string Core { get; set; } = "";

    [JsonPropertyName("port")]
    public int Port { get; set; }

    [JsonPropertyName("memory_mib")]
    public int MemoryMib { get; set; }

    [JsonPropertyName("auto_restart")]
    public bool AutoRestart { get; set; }

    [JsonPropertyName("eula_accepted")]
    public bool EulaAccepted { get; set; }

    [JsonPropertyName("node_id")]
    public string? NodeId { get; set; }

    [JsonPropertyName("desired_running")]
    public bool DesiredRunning { get; set; }

    [JsonPropertyName("group")]
    public string? Group { get; set; }

    [JsonPropertyName("tags")]
    public List<string>? Tags { get; set; }

    [JsonPropertyName("runtime")]
    public string? Runtime { get; set; }
}

public sealed class CocktailClient : ICocktailClient
{
    private readonly HttpClient _http;
    private readonly IReadOnlySet<string> _permissions;

    public CocktailClient(HttpClient http, IReadOnlySet<string> permissions)
    {
        _http = http;
        _permissions = permissions;
    }

    public async Task<IReadOnlyList<JsonInstance>> ListInstancesAsync(CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesRead);
        var list = await _http.GetFromJsonAsync<List<JsonInstance>>("/api/v1/instances", ct)
            ?? [];
        return list;
    }

    public async Task<JsonInstance?> GetInstanceAsync(string id, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesRead);
        using var res = await _http.GetAsync($"/api/v1/instances/{id}", ct);
        if (res.StatusCode == System.Net.HttpStatusCode.NotFound)
        {
            return null;
        }
        res.EnsureSuccessStatusCode();
        return await res.Content.ReadFromJsonAsync<JsonInstance>(ct);
    }

    public Task<JsonInstance> StartInstanceAsync(string id, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesWrite);
        return PostInstance($"/api/v1/instances/{id}/start", ct);
    }

    public Task<JsonInstance> StopInstanceAsync(string id, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesWrite);
        return PostInstance($"/api/v1/instances/{id}/stop", ct);
    }

    public async Task<IReadOnlyList<JsonNode>> ListNodesAsync(CancellationToken ct = default)
    {
        Ensure(PluginPermissions.NodesRead);
        var list = await _http.GetFromJsonAsync<List<JsonNode>>("/api/v1/nodes", ct) ?? [];
        return list;
    }

    public async Task<JsonInstance> CreateInstanceAsync(CreateInstanceBody body, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesWrite);
        using var res = await _http.PostAsJsonAsync("/api/v1/instances", body, ct);
        res.EnsureSuccessStatusCode();
        return await res.Content.ReadFromJsonAsync<JsonInstance>(ct)
            ?? throw new InvalidOperationException("empty instance response");
    }

    public async Task<JsonInstance> ApplySpecAsync(string id, string yaml, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesWrite);
        using var content = new StringContent(yaml, System.Text.Encoding.UTF8, "application/yaml");
        using var res = await _http.PutAsync($"/api/v1/instances/{id}/spec", content, ct);
        res.EnsureSuccessStatusCode();
        return await res.Content.ReadFromJsonAsync<JsonInstance>(ct)
            ?? throw new InvalidOperationException("empty instance response");
    }

    public async Task<IReadOnlyList<JsonPlugin>> ListPluginsAsync(string instanceId, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesRead);
        var list = await _http.GetFromJsonAsync<List<JsonPlugin>>($"/api/v1/instances/{instanceId}/plugins", ct)
            ?? [];
        return list;
    }

    public async Task EnablePluginAsync(string instanceId, string name, bool enabled, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.InstancesWrite);
        var action = enabled ? "enable" : "disable";
        using var res = await _http.PostAsync(
            $"/api/v1/instances/{instanceId}/plugins/{Uri.EscapeDataString(name)}/{action}",
            content: null,
            ct);
        res.EnsureSuccessStatusCode();
    }

    public async Task WriteFileAsync(string instanceId, string path, string content, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.FilesWrite);
        using var res = await _http.PutAsJsonAsync(
            $"/api/v1/instances/{instanceId}/files/content",
            new { path, content },
            ct);
        res.EnsureSuccessStatusCode();
    }

    public async Task<IReadOnlyList<JsonFileEntry>> ListFilesAsync(
        string instanceId,
        string path = "",
        CancellationToken ct = default)
    {
        EnsureAny(PluginPermissions.FilesRead, PluginPermissions.FilesWrite);
        var q = Uri.EscapeDataString(path);
        var list = await _http.GetFromJsonAsync<List<JsonFileEntry>>(
            $"/api/v1/instances/{instanceId}/files?path={q}", ct) ?? [];
        return list;
    }

    public async Task<string> ReadFileAsync(string instanceId, string path, CancellationToken ct = default)
    {
        EnsureAny(PluginPermissions.FilesRead, PluginPermissions.FilesWrite);
        var q = Uri.EscapeDataString(path);
        using var res = await _http.GetAsync(
            $"/api/v1/instances/{instanceId}/files/content?path={q}", ct);
        res.EnsureSuccessStatusCode();
        var body = await res.Content.ReadFromJsonAsync<FileContentBody>(ct);
        return body?.Content ?? "";
    }

    public async Task MkdirAsync(string instanceId, string path, CancellationToken ct = default)
    {
        Ensure(PluginPermissions.FilesWrite);
        using var res = await _http.PostAsJsonAsync(
            $"/api/v1/instances/{instanceId}/files/mkdir",
            new { path },
            ct);
        res.EnsureSuccessStatusCode();
    }

    public async Task UploadBytesAsync(
        string instanceId,
        string path,
        byte[] bytes,
        string fileName,
        CancellationToken ct = default)
    {
        Ensure(PluginPermissions.FilesWrite);
        using var content = new MultipartFormDataContent();
        var file = new ByteArrayContent(bytes);
        file.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/java-archive");
        content.Add(file, "file", fileName);
        var q = Uri.EscapeDataString(path);
        using var res = await _http.PostAsync(
            $"/api/v1/instances/{instanceId}/files/upload?path={q}",
            content,
            ct);
        res.EnsureSuccessStatusCode();
    }

    public async Task InstallModrinthAsync(
        string instanceId,
        string projectId,
        string? versionId = null,
        CancellationToken ct = default)
    {
        Ensure(PluginPermissions.FilesWrite);
        using var res = await _http.PostAsJsonAsync(
            $"/api/v1/instances/{instanceId}/modrinth/install",
            new { project_id = projectId, version_id = versionId, target = "plugin" },
            ct);
        res.EnsureSuccessStatusCode();
    }

    private sealed class FileContentBody
    {
        [JsonPropertyName("content")]
        public string Content { get; set; } = "";
    }

    private async Task<JsonInstance> PostInstance(string path, CancellationToken ct)
    {
        using var res = await _http.PostAsync(path, content: null, ct);
        res.EnsureSuccessStatusCode();
        return await res.Content.ReadFromJsonAsync<JsonInstance>(ct)
            ?? throw new InvalidOperationException("empty instance response");
    }

    private void Ensure(string permission)
    {
        if (!_permissions.Contains(permission))
        {
            throw new UnauthorizedAccessException($"plugin missing permission {permission}");
        }
    }

    private void EnsureAny(params string[] permissions)
    {
        if (permissions.Any(_permissions.Contains))
        {
            return;
        }

        throw new UnauthorizedAccessException($"plugin missing permission {permissions[0]}");
    }
}
