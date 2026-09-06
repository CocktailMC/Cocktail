using System.Text;
using System.Text.Json;

namespace Cocktail.Sdk;

public interface IPluginRouter
{
    void Map(string method, string path, Func<PluginHttpContext, Task<PluginHttpResult>> handler);
}

public sealed class PluginHttpContext
{
    public required string Method { get; init; }
    public required string Path { get; init; }
    public required IReadOnlyDictionary<string, string> Query { get; init; }
    public required byte[] Body { get; init; }

    public string Text => Encoding.UTF8.GetString(Body);

    public T? Json<T>() =>
        Body.Length == 0 ? default : JsonSerializer.Deserialize<T>(Body, PluginJson.Options);
}

public sealed class PluginHttpResult
{
    public int Status { get; init; } = 200;
    public string ContentType { get; init; } = "application/json; charset=utf-8";
    public byte[] Body { get; init; } = [];

    public static PluginHttpResult Json(object value, int status = 200) =>
        new()
        {
            Status = status,
            Body = JsonSerializer.SerializeToUtf8Bytes(value, PluginJson.Options),
        };

    public static PluginHttpResult Text(string value, int status = 200) =>
        new()
        {
            Status = status,
            ContentType = "text/plain; charset=utf-8",
            Body = Encoding.UTF8.GetBytes(value),
        };

    public static PluginHttpResult NotFound(string message = "not found") =>
        Json(new { error = message }, 404);

    public static PluginHttpResult BadRequest(string message) =>
        Json(new { error = message }, 400);
}

public static class PluginJson
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
    };
}
