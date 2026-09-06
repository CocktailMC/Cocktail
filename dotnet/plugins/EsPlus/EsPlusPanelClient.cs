using System.Net;
using System.Text.Json;

namespace Cocktail.Plugins.EsPlus;

internal sealed class EsPlusPanelClient : IDisposable
{
    private readonly HttpClient _http;
    private readonly CookieContainer _cookies = new();

    public EsPlusPanelClient(string baseUrl)
    {
        var handler = new HttpClientHandler
        {
            CookieContainer = _cookies,
            UseCookies = true,
            AllowAutoRedirect = false,
        };
        _http = new HttpClient(handler)
        {
            BaseAddress = new Uri(baseUrl.TrimEnd('/') + "/"),
            Timeout = TimeSpan.FromSeconds(8),
        };
        _http.DefaultRequestHeaders.TryAddWithoutValidation("User-Agent", "Cocktail-ESPlus-Adapter/0.1");
    }

    public async Task<bool> ProbeAsync(CancellationToken ct)
    {
        try
        {
            using var res = await _http.GetAsync("login", ct);
            return (int)res.StatusCode is >= 200 and < 500;
        }
        catch
        {
            return false;
        }
    }

    public async Task<(bool Ok, string? Error, bool Mfa)> LoginAsync(string username, string password, CancellationToken ct)
    {
        using var content = new FormUrlEncodedContent(new Dictionary<string, string>
        {
            ["username"] = username,
            ["password"] = password,
        });
        using var res = await _http.PostAsync("api/auth/login", content, ct);
        var text = await res.Content.ReadAsStringAsync(ct);
        if (!res.IsSuccessStatusCode)
        {
            return (false, $"login HTTP {(int)res.StatusCode}", false);
        }

        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            var ok = root.TryGetProperty("ok", out var okEl) && okEl.ValueKind == JsonValueKind.True;
            var mfa = root.TryGetProperty("mfaRequired", out var mfaEl) && mfaEl.ValueKind == JsonValueKind.True;
            var err = root.TryGetProperty("error", out var errEl) ? errEl.GetString() : null;
            return (ok, err, mfa);
        }
        catch
        {
            return (false, "login response was not json", false);
        }
    }

    public async Task<JsonElement?> GetJsonAsync(string relative, CancellationToken ct)
    {
        using var res = await _http.GetAsync(relative.TrimStart('/'), ct);
        if (!res.IsSuccessStatusCode)
        {
            return null;
        }

        await using var stream = await res.Content.ReadAsStreamAsync(ct);
        using var doc = await JsonDocument.ParseAsync(stream, cancellationToken: ct);
        return doc.RootElement.Clone();
    }

    public void Dispose() => _http.Dispose();
}
