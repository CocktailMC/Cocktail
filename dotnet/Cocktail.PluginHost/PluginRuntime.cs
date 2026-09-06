using System.Collections.Concurrent;
using System.Text.Json;
using Cocktail.Sdk;

namespace Cocktail.PluginHost;

internal sealed class PluginRuntime : IPluginContext, IPluginLog, IPluginRouter, IPluginEvents, IPluginScheduler, IDisposable
{
    private readonly ILogger _log;
    private readonly CancellationToken _shutdown;
    private readonly List<(string Method, string Path, Func<PluginHttpContext, Task<PluginHttpResult>> Handler)> _routes = [];
    private readonly List<Func<InstanceStatusEvent, CancellationToken, Task>> _status = [];
    private readonly List<Func<InstanceLogEvent, CancellationToken, Task>> _logs = [];
    private readonly List<Func<InstanceMetricEvent, CancellationToken, Task>> _metrics = [];
    private readonly List<PeriodicTimer> _timers = [];
    private readonly ConcurrentBag<Task> _loops = [];

    public PluginRuntime(
        PluginManifest manifest,
        string dataDirectory,
        ILogger log,
        CancellationToken shutdown)
    {
        Manifest = manifest;
        DataDirectory = dataDirectory;
        _log = log;
        _shutdown = shutdown;
        Permissions = manifest.Permissions.ToHashSet(StringComparer.OrdinalIgnoreCase);
        var plane = Environment.GetEnvironmentVariable("COCKTAIL_PLANE")?.TrimEnd('/')
                    ?? "http://127.0.0.1:11011";
        var http = new HttpClient { BaseAddress = new Uri(plane) };
        var token = Environment.GetEnvironmentVariable("COCKTAIL_API_TOKEN");
        if (string.IsNullOrEmpty(token))
        {
            token = Environment.GetEnvironmentVariable("COCKTAIL_PLUGIN_TOKEN");
        }
        if (!string.IsNullOrEmpty(token))
        {
            http.DefaultRequestHeaders.Authorization =
                new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);
        }
        ControlPlane = new CocktailClient(http, Permissions);
    }

    public PluginManifest Manifest { get; }
    public IReadOnlySet<string> Permissions { get; }
    public IPluginLog Log => this;
    public IPluginRouter Router => this;
    public IPluginEvents Events => this;
    public IPluginScheduler Scheduler => this;
    public ICocktailClient ControlPlane { get; }
    public string DataDirectory { get; }

    public bool Has(string permission) => Permissions.Contains(permission);

    public void Info(string message) =>
        _log.LogInformation("[{Plugin}] {Message}", Manifest.Id, message);

    public void Warn(string message) =>
        _log.LogWarning("[{Plugin}] {Message}", Manifest.Id, message);

    public void Error(string message, Exception? ex = null)
    {
        if (ex is null)
        {
            _log.LogError("[{Plugin}] {Message}", Manifest.Id, message);
        }
        else
        {
            _log.LogError(ex, "[{Plugin}] {Message}", Manifest.Id, message);
        }
    }

    public void Map(string method, string path, Func<PluginHttpContext, Task<PluginHttpResult>> handler)
    {
        Ensure(PluginPermissions.HttpExpose);
        var p = path.StartsWith('/') ? path : "/" + path;
        if (!p.Contains('*', StringComparison.Ordinal))
        {
            p = p.TrimEnd('/');
            if (p.Length == 0)
            {
                p = "/";
            }
        }
        _routes.Add((method.ToUpperInvariant(), p, handler));
    }

    public void OnStatus(Func<InstanceStatusEvent, CancellationToken, Task> handler)
    {
        Ensure(PluginPermissions.EventsSubscribe);
        _status.Add(handler);
    }

    public void OnLog(Func<InstanceLogEvent, CancellationToken, Task> handler)
    {
        Ensure(PluginPermissions.EventsLogs);
        _logs.Add(handler);
    }

    public void OnMetric(Func<InstanceMetricEvent, CancellationToken, Task> handler)
    {
        Ensure(PluginPermissions.EventsSubscribe);
        _metrics.Add(handler);
    }

    public void Every(TimeSpan period, Func<CancellationToken, Task> work)
    {
        Ensure(PluginPermissions.Scheduler);
        var timer = new PeriodicTimer(period);
        _timers.Add(timer);
        _loops.Add(Task.Run(async () =>
        {
            try
            {
                while (await timer.WaitForNextTickAsync(_shutdown))
                {
                    try
                    {
                        await work(_shutdown);
                    }
                    catch (Exception ex)
                    {
                        Error("scheduled work failed", ex);
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // host stopping
            }
        }, _shutdown));
    }

    public async Task DispatchAsync(string? type, JsonElement payload, CancellationToken ct)
    {
        switch (type)
        {
            case "status_changed":
                var st = new InstanceStatusEvent(
                    payload.GetProperty("instance_id").GetString() ?? "",
                    payload.GetProperty("status").GetString() ?? "",
                    payload.TryGetProperty("at", out var at)
                        ? at.GetDateTimeOffset()
                        : DateTimeOffset.UtcNow);
                foreach (var h in _status)
                {
                    await h(st, ct);
                }
                break;
            case "log":
                if (payload.TryGetProperty("line", out var line))
                {
                    var ev = new InstanceLogEvent(
                        payload.GetProperty("instance_id").GetString() ?? "",
                        line.TryGetProperty("stream", out var stream) ? stream.GetString() ?? "" : "",
                        line.TryGetProperty("line", out var text) ? text.GetString() ?? "" : "",
                        line.TryGetProperty("ts", out var ts) ? ts.GetDateTimeOffset() : DateTimeOffset.UtcNow);
                    foreach (var h in _logs)
                    {
                        await h(ev, ct);
                    }
                }
                break;
            case "metric":
                if (payload.TryGetProperty("sample", out var sample))
                {
                    var ev = new InstanceMetricEvent(
                        payload.GetProperty("instance_id").GetString() ?? "",
                        sample.TryGetProperty("ts", out var mts) ? mts.GetDateTimeOffset() : DateTimeOffset.UtcNow,
                        sample.TryGetProperty("cpu_pct", out var cpu) ? cpu.GetSingle() : 0,
                        sample.TryGetProperty("memory_mib", out var mem) ? mem.GetSingle() : 0,
                        sample.TryGetProperty("tps", out var tps) && tps.ValueKind is not JsonValueKind.Null
                            ? tps.GetSingle()
                            : null,
                        sample.TryGetProperty("players", out var players) ? players.GetUInt32() : 0);
                    foreach (var h in _metrics)
                    {
                        await h(ev, ct);
                    }
                }
                break;
        }
    }

    public async Task<PluginHttpResult> InvokeHttpAsync(
        string method,
        string path,
        IReadOnlyDictionary<string, string> query,
        byte[] body,
        CancellationToken ct)
    {
        var normalized = string.IsNullOrEmpty(path) ? "/" : (path.StartsWith('/') ? path : "/" + path);
        normalized = normalized.TrimEnd('/');
        if (normalized.Length == 0)
        {
            normalized = "/";
        }

        var candidates = _routes
            .Where(r => r.Method == method.ToUpperInvariant() && RouteMatches(r.Path, normalized))
            .OrderByDescending(r => r.Path.Length)
            .ToList();
        var match = candidates.FirstOrDefault();
        if (match.Handler is null)
        {
            return PluginHttpResult.NotFound($"no route {method} {normalized}");
        }

        var ctx = new PluginHttpContext
        {
            Method = method,
            Path = normalized,
            Query = query,
            Body = body,
        };
        return await match.Handler(ctx);
    }

    private static bool RouteMatches(string mapped, string actual)
    {
        if (mapped.EndsWith("/*", StringComparison.Ordinal))
        {
            var prefix = mapped[..^2];
            return actual.Equals(prefix, StringComparison.OrdinalIgnoreCase)
                   || actual.StartsWith(prefix + "/", StringComparison.OrdinalIgnoreCase);
        }

        return PathsEqual(mapped, actual);
    }

    private static bool PathsEqual(string mapped, string actual) =>
        string.Equals(mapped.TrimEnd('/').Length == 0 ? "/" : mapped.TrimEnd('/'), actual, StringComparison.OrdinalIgnoreCase);

    private void Ensure(string permission)
    {
        if (!Has(permission))
        {
            throw new UnauthorizedAccessException($"plugin {Manifest.Id} missing {permission}");
        }
    }

    public void Dispose()
    {
        foreach (var t in _timers)
        {
            t.Dispose();
        }
    }
}
