using System.Collections.Concurrent;
using System.Text.Json;
using Cocktail.Sdk;

namespace Cocktail.PluginHost;

internal sealed class PluginState
{
    public required PluginManifest Manifest { get; init; }
    public required string Directory { get; init; }
    public bool Enabled { get; set; } = true;
    public string? Error { get; set; }
    public bool Running { get; set; }
    internal PluginLoadContext? LoadContext { get; set; }
    internal ICocktailPlugin? Instance { get; set; }
    internal PluginRuntime? Runtime { get; set; }
}

internal sealed class PluginRegistry
{
    private readonly ILogger<PluginRegistry> _log;
    private readonly IHostApplicationLifetime _lifetime;
    private readonly ConcurrentDictionary<string, PluginState> _plugins = new(StringComparer.OrdinalIgnoreCase);
    private readonly string _statePath;

    public PluginRegistry(ILogger<PluginRegistry> log, IHostApplicationLifetime lifetime)
    {
        _log = log;
        _lifetime = lifetime;
        _statePath = Path.Combine(HostPaths.DataDir, "plugin-state.json");
    }

    public IReadOnlyCollection<PluginState> All => _plugins.Values.ToArray();

    public PluginState? Get(string id) =>
        _plugins.TryGetValue(id, out var p) ? p : null;

    public async Task DiscoverAndLoadAsync(CancellationToken ct)
    {
        var saved = LoadSaved();
        foreach (var dir in HostPaths.PluginSearchDirs())
        {
            if (!Directory.Exists(dir))
            {
                continue;
            }

            foreach (var pluginDir in Directory.GetDirectories(dir))
            {
                var manifestPath = Path.Combine(pluginDir, "plugin.json");
                if (!File.Exists(manifestPath))
                {
                    continue;
                }

                PluginManifest? manifest;
                try
                {
                    manifest = JsonSerializer.Deserialize<PluginManifest>(
                        await File.ReadAllTextAsync(manifestPath, ct),
                        PluginJson.Options);
                }
                catch (Exception ex)
                {
                    _log.LogError(ex, "invalid plugin.json in {Dir}", pluginDir);
                    continue;
                }

                if (manifest is null || string.IsNullOrWhiteSpace(manifest.Id))
                {
                    continue;
                }

                var state = new PluginState
                {
                    Manifest = manifest,
                    Directory = pluginDir,
                    Enabled = saved.GetValueOrDefault(manifest.Id, true),
                };
                _plugins[manifest.Id] = state;
                if (state.Enabled)
                {
                    await StartPluginAsync(state, ct);
                }
            }
        }

        _log.LogInformation("plugin host loaded {Count} plugin(s)", _plugins.Count);
    }

    public async Task StopAllAsync()
    {
        foreach (var p in _plugins.Values.ToArray())
        {
            await StopPluginAsync(p, CancellationToken.None);
        }
    }

    public async Task ReloadAsync(CancellationToken ct)
    {
        foreach (var p in _plugins.Values.ToArray())
        {
            await StopPluginAsync(p, ct);
        }
        _plugins.Clear();
        await DiscoverAndLoadAsync(ct);
    }

    public async Task SetEnabledAsync(string id, bool enabled, CancellationToken ct)
    {
        var state = Get(id) ?? throw new InvalidOperationException("plugin not found");
        state.Enabled = enabled;
        SaveEnabled();
        if (enabled)
        {
            await StartPluginAsync(state, ct);
        }
        else
        {
            await StopPluginAsync(state, ct);
        }
    }

    public async Task DispatchEventAsync(JsonElement payload, CancellationToken ct)
    {
        if (!payload.TryGetProperty("type", out var typeEl))
        {
            return;
        }

        var type = typeEl.GetString();
        foreach (var plugin in _plugins.Values)
        {
            if (!plugin.Enabled || plugin.Runtime is null)
            {
                continue;
            }

            try
            {
                await plugin.Runtime.DispatchAsync(type, payload, ct);
            }
            catch (Exception ex)
            {
                plugin.Error = ex.Message;
                _log.LogError(ex, "plugin {Id} failed handling {Type}", plugin.Manifest.Id, type);
            }
        }
    }

    public async Task<PluginHttpResult?> HandleHttpAsync(
        string pluginId,
        string method,
        string path,
        IReadOnlyDictionary<string, string> query,
        byte[] body,
        CancellationToken ct)
    {
        var state = Get(pluginId);
        if (state is null)
        {
            return PluginHttpResult.NotFound("plugin not found");
        }

        if (!state.Enabled || state.Runtime is null)
        {
            return PluginHttpResult.Json(new { error = "plugin disabled" }, 409);
        }

        if (!state.Runtime.Has(PluginPermissions.HttpExpose))
        {
            return PluginHttpResult.Json(new { error = "plugin has no http.expose" }, 403);
        }

        return await state.Runtime.InvokeHttpAsync(method, path, query, body, ct);
    }

    private async Task StartPluginAsync(PluginState state, CancellationToken ct)
    {
        await StopPluginAsync(state, ct);
        state.Error = null;
        try
        {
            var assemblyName = state.Manifest.EntryAssembly
                ?? Directory.GetFiles(state.Directory, "*.dll")
                    .Select(Path.GetFileName)
                    .FirstOrDefault(n =>
                        n is not null
                        && !n.Equals("Cocktail.Sdk.dll", StringComparison.OrdinalIgnoreCase)
                        && !n.StartsWith("Microsoft.", StringComparison.OrdinalIgnoreCase)
                        && !n.StartsWith("System.", StringComparison.OrdinalIgnoreCase));
            if (string.IsNullOrEmpty(assemblyName))
            {
                throw new InvalidOperationException("no plugin assembly in directory");
            }

            var dll = Path.GetFullPath(Path.Combine(state.Directory, assemblyName));
            if (!File.Exists(dll))
            {
                throw new FileNotFoundException("plugin assembly missing", dll);
            }

            var alc = new PluginLoadContext(dll);
            var asm = alc.LoadFromAssemblyPath(dll);
            var type = FindEntryType(asm, state.Manifest.EntryType)
                ?? throw new InvalidOperationException("no ICocktailPlugin type found");
            if (Activator.CreateInstance(type) is not ICocktailPlugin plugin)
            {
                throw new InvalidOperationException($"{type.FullName} is not ICocktailPlugin");
            }

            var dataDir = Path.Combine(HostPaths.DataDir, "plugin-data", state.Manifest.Id);
            Directory.CreateDirectory(dataDir);
            var runtime = new PluginRuntime(state.Manifest, dataDir, _log, _lifetime.ApplicationStopping);
            await plugin.StartAsync(runtime, ct);
            state.LoadContext = alc;
            state.Instance = plugin;
            state.Runtime = runtime;
            state.Running = true;
            _log.LogInformation("started plugin {Id} {Name}", state.Manifest.Id, state.Manifest.Name);
        }
        catch (Exception ex)
        {
            state.Error = ex.Message;
            state.Running = false;
            _log.LogError(ex, "failed to start plugin {Id}", state.Manifest.Id);
        }
    }

    private async Task StopPluginAsync(PluginState state, CancellationToken ct)
    {
        try
        {
            if (state.Instance is not null)
            {
                await state.Instance.StopAsync(ct);
            }
        }
        catch (Exception ex)
        {
            _log.LogWarning(ex, "plugin {Id} stop failed", state.Manifest.Id);
        }

        state.Runtime?.Dispose();
        state.Runtime = null;
        state.Instance = null;
        state.Running = false;
        var alc = state.LoadContext;
        state.LoadContext = null;
        if (alc is not null)
        {
            alc.Unload();
        }
    }

    private static Type? FindEntryType(System.Reflection.Assembly asm, string? entryType)
    {
        if (!string.IsNullOrWhiteSpace(entryType))
        {
            return asm.GetType(entryType, throwOnError: false);
        }

        return asm.DefinedTypes
            .Select(t => t.AsType())
            .FirstOrDefault(t =>
                typeof(ICocktailPlugin).IsAssignableFrom(t) && t is { IsAbstract: false, IsInterface: false });
    }

    private Dictionary<string, bool> LoadSaved()
    {
        try
        {
            if (!File.Exists(_statePath))
            {
                return new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
            }

            return JsonSerializer.Deserialize<Dictionary<string, bool>>(
                       File.ReadAllText(_statePath), PluginJson.Options)
                   ?? new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        }
        catch
        {
            return new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        }
    }

    private void SaveEnabled()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_statePath)!);
        var map = _plugins.ToDictionary(kv => kv.Key, kv => kv.Value.Enabled, StringComparer.OrdinalIgnoreCase);
        File.WriteAllText(_statePath, JsonSerializer.Serialize(map, PluginJson.Options));
    }
}
