using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using Cocktail.Sdk;

namespace Cocktail.Plugins.EsPlus;

public sealed class EsPlusPlugin : ICocktailPlugin
{
    internal const string Example = """
        # ESPlus is a NeoForge 1.21.1 server+client mod (LGPL-3.0). Cocktail does not vendor the repo.
        # 1) Create a NeoForge instance, then POST /install on this plugin (or drop esplus-*.jar into mods/).
        # 2) POST /ensure-config so panelPassword is not the default "esplus" and bind stays 127.0.0.1.
        # 3) Restart the instance; panel listens on the assigned port (default base 8088).
        #
        # GameOps Network sketch (core must be NeoForge, not Paper):
        apiVersion: cocktail.gameops/v1
        kind: Network
        metadata:
          name: esplus-smp
        spec:
          desiredRunning: true
          servers:
            - name: esplus-1
              nodeId: local
              core: neoforge
              memoryMib: 4096
              port: 25565
              group: esplus
        """;

    private IPluginContext? _ctx;
    private AdapterConfig _cfg = new();

    public Task StartAsync(IPluginContext context, CancellationToken cancellationToken)
    {
        _ctx = context;
        _cfg = LoadConfig(context);
        context.Log.Info($"ESPlus adapter online (repo={_cfg.GithubRepo})");

        context.Router.Map("GET", "/summary", async _ =>
        {
            var fleet = await ScanAsync(context, CancellationToken.None);
            return PluginHttpResult.Json(new
            {
                plugin = context.Manifest.Id,
                source = "https://github.com/FORGE24/ESPlus",
                githubRepo = _cfg.GithubRepo,
                notes = new[]
                {
                    "Server and client both need the same ESPlus fat jar; do not ship panel.jar separately.",
                    "Panel bind should stay 127.0.0.1. Default password esplus refuses to start unless explicitly allowed.",
                    "Set COCKTAIL_ESPLUS_JAR to a local fat jar if GitHub Releases are empty.",
                },
                instances = fleet,
            });
        });

        context.Router.Map("GET", "/example", _ =>
            Task.FromResult(PluginHttpResult.Text(Example)));

        context.Router.Map("POST", "/config", http =>
        {
            var body = http.Json<AdapterConfigPatch>();
            if (body is null)
            {
                return Task.FromResult(PluginHttpResult.BadRequest("invalid json"));
            }

            if (!string.IsNullOrWhiteSpace(body.GithubRepo))
            {
                _cfg.GithubRepo = body.GithubRepo.Trim();
            }

            if (body.JarPath is not null)
            {
                _cfg.JarPath = body.JarPath;
            }

            SaveConfig(context);
            return Task.FromResult(PluginHttpResult.Json(new { ok = true, githubRepo = _cfg.GithubRepo }));
        });

        context.Router.Map("POST", "/ensure-config", async http =>
        {
            var body = http.Json<InstanceAction>();
            if (string.IsNullOrWhiteSpace(body?.InstanceId))
            {
                return PluginHttpResult.BadRequest("instanceId required");
            }

            try
            {
                var result = await EnsureConfigAsync(context, body, CancellationToken.None);
                return PluginHttpResult.Json(result);
            }
            catch (Exception ex)
            {
                context.Log.Error("ensure-config failed", ex);
                return PluginHttpResult.BadRequest(ex.Message);
            }
        });

        context.Router.Map("POST", "/install", async http =>
        {
            var body = http.Json<InstanceAction>();
            if (string.IsNullOrWhiteSpace(body?.InstanceId))
            {
                return PluginHttpResult.BadRequest("instanceId required");
            }

            try
            {
                var (bytes, fileName) = await EsPlusJar.ResolveAsync(
                    context.DataDirectory, _cfg.GithubRepo, _cfg.JarPath, CancellationToken.None);
                await context.ControlPlane.MkdirAsync(body.InstanceId, "mods");
                await context.ControlPlane.UploadBytesAsync(
                    body.InstanceId, $"mods/{fileName}", bytes, fileName);
                var cfg = await EnsureConfigAsync(context, body, CancellationToken.None);
                return PluginHttpResult.Json(new { ok = true, jar = fileName, config = cfg });
            }
            catch (Exception ex)
            {
                context.Log.Error("install failed", ex);
                return PluginHttpResult.BadRequest(ex.Message);
            }
        });

        context.Router.Map("GET", "/panel/*", async http =>
        {
            var id = EsPlusIds.InstanceIdFrom(http.Path, "/panel/");
            if (string.IsNullOrWhiteSpace(id))
            {
                return PluginHttpResult.BadRequest("instance id required");
            }

            var kind = EsPlusIds.TailAfter(http.Path, id) ?? "dashboard";
            var allowed = kind is "dashboard" or "alerts" or "audit" or "players";
            if (!allowed)
            {
                return PluginHttpResult.BadRequest("kind must be dashboard, alerts, audit, or players");
            }

            try
            {
                var json = await FetchPanelAsync(context, id, kind, CancellationToken.None);
                return json is null
                    ? PluginHttpResult.Json(new { error = "panel unreachable or login failed" }, 502)
                    : PluginHttpResult.Json(json);
            }
            catch (Exception ex)
            {
                return PluginHttpResult.BadRequest(ex.Message);
            }
        });

        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private async Task<List<object>> ScanAsync(IPluginContext ctx, CancellationToken ct)
    {
        IReadOnlyList<JsonInstance> instances;
        try
        {
            instances = await ctx.ControlPlane.ListInstancesAsync(ct);
        }
        catch (Exception ex)
        {
            ctx.Log.Warn($"list instances failed: {ex.Message}");
            return [];
        }

        var rows = new List<object>();
        foreach (var inst in instances)
        {
            var secrets = _cfg.Instance(inst.Id);
            string? jar = null;
            string? toml = null;
            var configPresent = false;
            try
            {
                var mods = await ListSafe(ctx, inst.Id, "mods", ct);
                jar = mods.FirstOrDefault(e => !e.IsDir && EsPlusJar.IsModJar(e.Name))?.Name;
            }
            catch
            {
                // mods/ may not exist
            }

            try
            {
                toml = await ctx.ControlPlane.ReadFileAsync(inst.Id, EsPlusToml.ConfigPath, ct);
                configPresent = true;
            }
            catch
            {
                configPresent = false;
            }

            var port = secrets.PanelPort
                       ?? (toml is null ? 8088 : EsPlusToml.ReadInt(toml, "panelPort", 8088));
            var bind = toml is null ? "127.0.0.1" : EsPlusToml.ReadString(toml, "panelBindAddress", "127.0.0.1");
            var enabled = toml is null || EsPlusToml.ReadBool(toml, "panelEnabled", true);
            var username = toml is null
                ? EsPlusToml.DefaultUsername
                : EsPlusToml.ReadString(toml, "panelUsername", EsPlusToml.DefaultUsername);
            var password = toml is null
                ? ""
                : EsPlusToml.ReadString(toml, "panelPassword", "");
            var defaultPw = string.Equals(password, EsPlusToml.DefaultPasswordLiteral, StringComparison.Ordinal);
            var panelUrl = $"http://{bind}:{port}/";

            bool? reachable = null;
            if (jar is not null && enabled && inst.Status is "running" or "online")
            {
                using var probe = new EsPlusPanelClient(panelUrl);
                reachable = await probe.ProbeAsync(ct);
            }

            rows.Add(new
            {
                id = inst.Id,
                name = inst.Spec.Name,
                status = inst.Status,
                core = inst.Spec.Core,
                installed = jar is not null,
                jar,
                configPresent,
                panelEnabled = enabled,
                panelBind = bind,
                panelPort = port,
                panelUrl,
                panelReachable = reachable,
                panelUsername = username,
                passwordIsDefault = defaultPw,
                passwordConfigured = secrets.PasswordSet || (!string.IsNullOrEmpty(password) && !defaultPw),
            });
        }

        return rows;
    }

    private async Task<object> EnsureConfigAsync(IPluginContext ctx, InstanceAction body, CancellationToken ct)
    {
        var inst = await ctx.ControlPlane.GetInstanceAsync(body.InstanceId, ct)
                   ?? throw new InvalidOperationException("instance not found");

        var used = new List<int>();
        foreach (var other in _cfg.Instances)
        {
            if (other.Key != body.InstanceId && other.Value.PanelPort is int p)
            {
                used.Add(p);
            }
        }

        string? toml = null;
        try
        {
            toml = await ctx.ControlPlane.ReadFileAsync(body.InstanceId, EsPlusToml.ConfigPath, ct);
        }
        catch
        {
            // first write
        }

        var secrets = _cfg.Instance(body.InstanceId);
        var port = body.PanelPort
                   ?? secrets.PanelPort
                   ?? (toml is null ? EsPlusPorts.Suggest(body.InstanceId, used) : EsPlusToml.ReadInt(toml, "panelPort", 8088));
        var username = string.IsNullOrWhiteSpace(body.Username)
            ? (secrets.Username ?? EsPlusToml.DefaultUsername)
            : body.Username.Trim();
        var generated = false;
        var password = body.Password;
        if (string.IsNullOrWhiteSpace(password))
        {
            password = secrets.Password;
        }

        if (string.IsNullOrWhiteSpace(password)
            || string.Equals(password, EsPlusToml.DefaultPasswordLiteral, StringComparison.Ordinal))
        {
            password = Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant();
            generated = true;
        }

        var next = EsPlusToml.ApplyPanel(toml ?? "", port, "127.0.0.1", username, password, body.PanelEnabled ?? true);
        await ctx.ControlPlane.MkdirAsync(body.InstanceId, "config");
        await ctx.ControlPlane.WriteFileAsync(body.InstanceId, EsPlusToml.ConfigPath, next);

        secrets.Username = username;
        secrets.Password = password;
        secrets.PasswordSet = true;
        secrets.PanelPort = port;
        SaveConfig(ctx);

        return new
        {
            ok = true,
            instanceId = inst.Id,
            panelPort = port,
            panelBind = "127.0.0.1",
            panelUsername = username,
            passwordGenerated = generated,
            panelPassword = generated ? password : null,
            hint = generated
                ? "panelPassword was generated and written to config/esplus-common.toml; restart the instance to apply."
                : "panel config updated; restart the instance if it is already running.",
        };
    }

    private async Task<object?> FetchPanelAsync(IPluginContext ctx, string instanceId, string kind, CancellationToken ct)
    {
        var secrets = _cfg.Instance(instanceId);
        string? toml = null;
        try
        {
            toml = await ctx.ControlPlane.ReadFileAsync(instanceId, EsPlusToml.ConfigPath, ct);
        }
        catch
        {
            // ignore
        }

        var port = secrets.PanelPort
                   ?? (toml is null ? 8088 : EsPlusToml.ReadInt(toml, "panelPort", 8088));
        var bind = toml is null ? "127.0.0.1" : EsPlusToml.ReadString(toml, "panelBindAddress", "127.0.0.1");
        var username = secrets.Username
                       ?? (toml is null ? EsPlusToml.DefaultUsername : EsPlusToml.ReadString(toml, "panelUsername", EsPlusToml.DefaultUsername));
        var password = secrets.Password
                       ?? (toml is null ? "" : EsPlusToml.ReadString(toml, "panelPassword", ""));
        if (string.IsNullOrEmpty(password))
        {
            throw new InvalidOperationException("no panel password stored; run ensure-config first");
        }

        var url = $"http://{bind}:{port}/";
        using var client = new EsPlusPanelClient(url);
        if (!await client.ProbeAsync(ct))
        {
            return null;
        }

        var (ok, err, mfa) = await client.LoginAsync(username, password, ct);
        if (mfa)
        {
            throw new InvalidOperationException("panel MFA is enabled; complete login in the ESPlus UI");
        }

        if (!ok)
        {
            throw new InvalidOperationException(err ?? "panel login failed");
        }

        var path = kind switch
        {
            "alerts" => "api/alerts",
            "audit" => "api/audit",
            "players" => "api/players/online",
            _ => "api/dashboard",
        };
        var json = await client.GetJsonAsync(path, ct);
        return json is { } el
            ? JsonSerializer.Deserialize<object>(el.GetRawText(), PluginJson.Options)
            : null;
    }

    private static async Task<IReadOnlyList<JsonFileEntry>> ListSafe(
        IPluginContext ctx, string id, string path, CancellationToken ct)
    {
        try
        {
            return await ctx.ControlPlane.ListFilesAsync(id, path, ct);
        }
        catch
        {
            return [];
        }
    }

    private static AdapterConfig LoadConfig(IPluginContext ctx)
    {
        var path = Path.Combine(ctx.DataDirectory, "config.json");
        if (!File.Exists(path))
        {
            return new AdapterConfig();
        }

        try
        {
            return JsonSerializer.Deserialize<AdapterConfig>(File.ReadAllText(path), PluginJson.Options)
                   ?? new AdapterConfig();
        }
        catch
        {
            return new AdapterConfig();
        }
    }

    private void SaveConfig(IPluginContext ctx)
    {
        Directory.CreateDirectory(ctx.DataDirectory);
        File.WriteAllText(
            Path.Combine(ctx.DataDirectory, "config.json"),
            JsonSerializer.Serialize(_cfg, PluginJson.Options));
    }
}

internal sealed class AdapterConfig
{
    public string GithubRepo { get; set; } = "FORGE24/ESPlus";
    public string? JarPath { get; set; }
    public Dictionary<string, InstanceSecrets> Instances { get; set; } = new(StringComparer.OrdinalIgnoreCase);

    public InstanceSecrets Instance(string id)
    {
        if (!Instances.TryGetValue(id, out var s))
        {
            s = new InstanceSecrets();
            Instances[id] = s;
        }

        return s;
    }
}

internal sealed class InstanceSecrets
{
    public string? Username { get; set; }
    public string? Password { get; set; }
    public bool PasswordSet { get; set; }
    public int? PanelPort { get; set; }
}

internal sealed class AdapterConfigPatch
{
    public string? GithubRepo { get; set; }
    public string? JarPath { get; set; }
}

internal sealed class InstanceAction
{
    [JsonPropertyName("instanceId")]
    public string InstanceId { get; set; } = "";

    public string? Username { get; set; }
    public string? Password { get; set; }
    public int? PanelPort { get; set; }
    public bool? PanelEnabled { get; set; }
}
