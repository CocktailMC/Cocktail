using System.Net.Http.Headers;
using System.Text.Json;

namespace Cocktail.Plugins.EsPlus;

internal static class EsPlusJar
{
    public static bool IsModJar(string name)
    {
        var n = name.ToLowerInvariant();
        if (!n.StartsWith("esplus", StringComparison.Ordinal) || !n.EndsWith(".jar", StringComparison.Ordinal))
        {
            return false;
        }

        return !n.Contains("sources", StringComparison.Ordinal)
               && !n.Contains("javadoc", StringComparison.Ordinal)
               && !n.Contains("panel", StringComparison.Ordinal);
    }

    public static async Task<(byte[] Bytes, string FileName)> ResolveAsync(
        string dataDir,
        string githubRepo,
        string? jarPathOverride,
        CancellationToken ct)
    {
        var env = Environment.GetEnvironmentVariable("COCKTAIL_ESPLUS_JAR");
        foreach (var candidate in new[] { jarPathOverride, env, Path.Combine(dataDir, "vendor", "esplus.jar") })
        {
            if (!string.IsNullOrWhiteSpace(candidate) && File.Exists(candidate))
            {
                var name = Path.GetFileName(candidate);
                if (!IsModJar(name))
                {
                    name = "esplus.jar";
                }

                return (await File.ReadAllBytesAsync(candidate, ct), name);
            }
        }

        using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(3) };
        http.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("Cocktail-ESPlus-Adapter", "0.1"));
        http.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("application/vnd.github+json"));

        var url = $"https://api.github.com/repos/{githubRepo}/releases/latest";
        using var latest = await http.GetAsync(url, ct);
        JsonElement root;
        if (latest.IsSuccessStatusCode)
        {
            root = JsonDocument.Parse(await latest.Content.ReadAsStringAsync(ct)).RootElement.Clone();
        }
        else
        {
            using var list = await http.GetAsync($"https://api.github.com/repos/{githubRepo}/releases?per_page=5", ct);
            list.EnsureSuccessStatusCode();
            using var doc = JsonDocument.Parse(await list.Content.ReadAsStringAsync(ct));
            if (doc.RootElement.GetArrayLength() == 0)
            {
                throw new InvalidOperationException(
                    $"no GitHub releases on {githubRepo}; set COCKTAIL_ESPLUS_JAR or copy the fat jar to {Path.Combine(dataDir, "vendor", "esplus.jar")}");
            }

            root = doc.RootElement[0].Clone();
        }

        if (!root.TryGetProperty("assets", out var assets) || assets.ValueKind != JsonValueKind.Array)
        {
            throw new InvalidOperationException("GitHub release has no assets");
        }

        string? download = null;
        string fileName = "esplus.jar";
        foreach (var asset in assets.EnumerateArray())
        {
            var name = asset.GetProperty("name").GetString() ?? "";
            if (!IsModJar(name))
            {
                continue;
            }

            download = asset.GetProperty("browser_download_url").GetString();
            fileName = name;
            break;
        }

        if (string.IsNullOrEmpty(download))
        {
            throw new InvalidOperationException(
                "release has no esplus-*.jar asset (panel.jar is embedded in the mod and must not be installed separately)");
        }

        using var jarRes = await http.GetAsync(download, ct);
        jarRes.EnsureSuccessStatusCode();
        var bytes = await jarRes.Content.ReadAsByteArrayAsync(ct);
        var vendor = Path.Combine(dataDir, "vendor");
        Directory.CreateDirectory(vendor);
        await File.WriteAllBytesAsync(Path.Combine(vendor, fileName), bytes, ct);
        return (bytes, fileName);
    }
}

internal static class EsPlusPorts
{
    public static int Suggest(string instanceId, IEnumerable<int> used)
    {
        var taken = used.ToHashSet();
        var seed = Math.Abs(instanceId.GetHashCode(StringComparison.Ordinal)) % 120;
        for (var i = 0; i < 120; i++)
        {
            var port = 8088 + ((seed + i) % 120);
            if (!taken.Contains(port))
            {
                return port;
            }
        }

        return 8088;
    }
}

internal static class EsPlusIds
{
    public static string? InstanceIdFrom(string path, string prefix)
    {
        if (!path.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        var rest = path[prefix.Length..];
        var slash = rest.IndexOf('/');
        return slash < 0 ? rest : rest[..slash];
    }

    public static string? TailAfter(string path, string instanceId)
    {
        var marker = "/" + instanceId + "/";
        var i = path.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
        return i < 0 ? null : path[(i + marker.Length)..];
    }
}
