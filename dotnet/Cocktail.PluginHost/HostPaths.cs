namespace Cocktail.PluginHost;

internal static class HostPaths
{
    public static string DataDir
    {
        get
        {
            var env = Environment.GetEnvironmentVariable("COCKTAIL_DATA");
            if (!string.IsNullOrWhiteSpace(env))
            {
                return env;
            }

            return Path.GetFullPath("data");
        }
    }

    public static IEnumerable<string> PluginSearchDirs()
    {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var env = Environment.GetEnvironmentVariable("COCKTAIL_PLUGIN_DIR");
        var list = new List<string>();
        if (!string.IsNullOrWhiteSpace(env))
        {
            list.Add(env);
        }

        list.Add(Path.Combine(DataDir, "extensions"));
        list.Add(Path.GetFullPath("dotnet/dist/plugins"));
        list.Add(Path.GetFullPath("dist/plugins"));
        if (AppContext.BaseDirectory is { } baseDir)
        {
            list.Add(Path.Combine(baseDir, "plugins"));
        }

        foreach (var p in list)
        {
            var full = Path.GetFullPath(p);
            if (seen.Add(full))
            {
                yield return full;
            }
        }
    }
}
