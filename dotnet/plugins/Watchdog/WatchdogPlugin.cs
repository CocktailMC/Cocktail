using System.Collections.Concurrent;
using System.Text.Json;
using Cocktail.Sdk;

namespace Cocktail.Plugins.Watchdog;

public sealed class WatchdogPlugin : ICocktailPlugin
{
    private readonly ConcurrentQueue<Incident> _incidents = new();
    private IPluginContext? _ctx;
    private int _max = 200;
    private bool _autoStartOnCrash;

    public Task StartAsync(IPluginContext context, CancellationToken cancellationToken)
    {
        _ctx = context;
        LoadConfig(context);
        context.Log.Info($"watchdog online (autoStartOnCrash={_autoStartOnCrash})");

        context.Events.OnStatus(async (ev, ct) =>
        {
            if (!string.Equals(ev.Status, "crashed", StringComparison.OrdinalIgnoreCase))
            {
                return;
            }

            var incident = new Incident(ev.InstanceId, ev.At, ev.Status, "status crashed");
            _incidents.Enqueue(incident);
            Trim();
            Persist(context);
            context.Log.Warn($"crash recorded for {ev.InstanceId}");

            if (_autoStartOnCrash)
            {
                try
                {
                    await context.ControlPlane.StartInstanceAsync(ev.InstanceId, ct);
                    context.Log.Info($"requested start after crash: {ev.InstanceId}");
                }
                catch (Exception ex)
                {
                    context.Log.Error("auto-start after crash failed", ex);
                }
            }
        });

        context.Router.Map("GET", "/summary", _ =>
        {
            var items = _incidents.ToArray().Reverse().Take(50).ToArray();
            return Task.FromResult(PluginHttpResult.Json(new
            {
                plugin = context.Manifest.Id,
                autoStartOnCrash = _autoStartOnCrash,
                total = _incidents.Count,
                incidents = items,
            }));
        });

        context.Router.Map("POST", "/config", async http =>
        {
            var cfg = http.Json<WatchdogConfig>();
            if (cfg is null)
            {
                return PluginHttpResult.BadRequest("invalid json");
            }

            _autoStartOnCrash = cfg.AutoStartOnCrash;
            if (cfg.MaxIncidents is > 10 and < 5000)
            {
                _max = cfg.MaxIncidents.Value;
            }

            await File.WriteAllTextAsync(
                ConfigPath(context),
                JsonSerializer.Serialize(new WatchdogConfig(_autoStartOnCrash, _max), PluginJson.Options),
                CancellationToken.None);
            return PluginHttpResult.Json(new { ok = true, autoStartOnCrash = _autoStartOnCrash, max = _max });
        });

        context.Scheduler.Every(TimeSpan.FromMinutes(10), _ =>
        {
            Trim();
            Persist(context);
            return Task.CompletedTask;
        });

        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken)
    {
        if (_ctx is not null)
        {
            Persist(_ctx);
        }

        return Task.CompletedTask;
    }

    private void LoadConfig(IPluginContext ctx)
    {
        var path = ConfigPath(ctx);
        if (!File.Exists(path))
        {
            return;
        }

        try
        {
            var cfg = JsonSerializer.Deserialize<WatchdogConfig>(File.ReadAllText(path), PluginJson.Options);
            if (cfg is null)
            {
                return;
            }

            _autoStartOnCrash = cfg.AutoStartOnCrash;
            if (cfg.MaxIncidents is > 10)
            {
                _max = cfg.MaxIncidents.Value;
            }
        }
        catch
        {
            // keep defaults
        }

        var store = Path.Combine(ctx.DataDirectory, "incidents.json");
        if (!File.Exists(store))
        {
            return;
        }

        try
        {
            var items = JsonSerializer.Deserialize<List<Incident>>(File.ReadAllText(store), PluginJson.Options);
            if (items is null)
            {
                return;
            }

            foreach (var i in items)
            {
                _incidents.Enqueue(i);
            }
        }
        catch
        {
            // ignore corrupt store
        }
    }

    private void Persist(IPluginContext ctx)
    {
        var store = Path.Combine(ctx.DataDirectory, "incidents.json");
        File.WriteAllText(store, JsonSerializer.Serialize(_incidents.ToArray(), PluginJson.Options));
    }

    private void Trim()
    {
        while (_incidents.Count > _max && _incidents.TryDequeue(out _))
        {
        }
    }

    private static string ConfigPath(IPluginContext ctx) => Path.Combine(ctx.DataDirectory, "config.json");
}

internal sealed record Incident(string InstanceId, DateTimeOffset At, string Status, string Note);

internal sealed record WatchdogConfig(bool AutoStartOnCrash, int? MaxIncidents);
