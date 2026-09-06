using System.Text.Json;
using Cocktail.PluginHost;
using Cocktail.Sdk;

var builder = WebApplication.CreateBuilder(args);
builder.Logging.AddSimpleConsole(o => o.SingleLine = true);
builder.Services.AddSingleton<PluginRegistry>();

var app = builder.Build();
var registry = app.Services.GetRequiredService<PluginRegistry>();
await registry.DiscoverAndLoadAsync(CancellationToken.None);

app.Lifetime.ApplicationStopping.Register(() =>
{
    registry.StopAllAsync().GetAwaiter().GetResult();
});

app.MapGet("/health", (PluginRegistry plugins) =>
{
    var list = plugins.All;
    return Results.Json(new
    {
        name = "cocktail-plugin-host",
        status = "ok",
        plugins = list.Count,
        running = list.Count(p => p.Running),
    }, PluginJson.Options);
});

app.MapGet("/v1/catalog", (PluginRegistry plugins) =>
{
    var items = plugins.All.Select(p => new
    {
        id = p.Manifest.Id,
        name = p.Manifest.Name,
        version = p.Manifest.Version,
        description = p.Manifest.Description,
        permissions = p.Manifest.Permissions,
        ui = p.Manifest.Ui,
        enabled = p.Enabled,
        running = p.Running,
        error = p.Error,
        directory = p.Directory,
    });
    return Results.Json(new { items }, PluginJson.Options);
});

app.MapPost("/v1/reload", async (PluginRegistry plugins, CancellationToken ct) =>
{
    await plugins.ReloadAsync(ct);
    return Results.Json(new { ok = true, plugins = plugins.All.Count }, PluginJson.Options);
});

app.MapPut("/v1/plugins/{id}/enabled", async (string id, EnabledBody body, PluginRegistry plugins, CancellationToken ct) =>
{
    try
    {
        await plugins.SetEnabledAsync(id, body.Enabled, ct);
        return Results.Json(new { ok = true, id, enabled = body.Enabled }, PluginJson.Options);
    }
    catch (InvalidOperationException ex)
    {
        return Results.Json(new { error = ex.Message }, statusCode: 404);
    }
});

app.MapPost("/v1/events", async (HttpRequest req, PluginRegistry plugins, CancellationToken ct) =>
{
    if (!PluginAuth.Ok(req))
    {
        return Results.Unauthorized();
    }

    using var doc = await JsonDocument.ParseAsync(req.Body, cancellationToken: ct);
    await plugins.DispatchEventAsync(doc.RootElement.Clone(), ct);
    return Results.Json(new { ok = true }, PluginJson.Options);
});

app.MapMethods("/v1/ext/{pluginId}/{**rest}", ["GET", "POST", "PUT", "PATCH", "DELETE"], async (
    string pluginId,
    string? rest,
    HttpRequest req,
    PluginRegistry plugins,
    CancellationToken ct) =>
{
    if (!PluginAuth.Ok(req))
    {
        return Results.Unauthorized();
    }

    using var ms = new MemoryStream();
    await req.Body.CopyToAsync(ms, ct);
    var query = req.Query.ToDictionary(
        kv => kv.Key,
        kv => kv.Value.ToString(),
        StringComparer.OrdinalIgnoreCase);
    var result = await plugins.HandleHttpAsync(
        pluginId,
        req.Method,
        "/" + (rest ?? ""),
        query,
        ms.ToArray(),
        ct);
    if (result is null)
    {
        return Results.NotFound();
    }

    return new RawBytesResult(result.Status, result.ContentType, result.Body);
});

var url = Environment.GetEnvironmentVariable("COCKTAIL_PLUGIN_BIND");
if (string.IsNullOrWhiteSpace(url))
{
    url = "http://127.0.0.1:11012";
}

if (!url.StartsWith("http", StringComparison.OrdinalIgnoreCase))
{
    url = "http://" + url;
}

app.Logger.LogInformation("Cocktail plugin host listening on {Url}", url);
app.Run(url);

internal sealed record EnabledBody(bool Enabled);

internal sealed class RawBytesResult(int status, string contentType, byte[] body) : IResult
{
    public async Task ExecuteAsync(HttpContext http)
    {
        http.Response.StatusCode = status;
        http.Response.ContentType = contentType;
        await http.Response.Body.WriteAsync(body);
    }
}

internal static class PluginAuth
{
    public static bool Ok(HttpRequest req)
    {
        var expected = Environment.GetEnvironmentVariable("COCKTAIL_PLUGIN_TOKEN");
        if (string.IsNullOrEmpty(expected))
        {
            return true;
        }

        if (req.Headers.TryGetValue("X-Cocktail-Plugin", out var header) && header == expected)
        {
            return true;
        }

        var auth = req.Headers.Authorization.ToString();
        return auth.Equals($"Bearer {expected}", StringComparison.Ordinal);
    }
}
