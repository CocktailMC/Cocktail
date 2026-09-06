using System.Text.RegularExpressions;

namespace Cocktail.Plugins.EsPlus;

internal static class EsPlusToml
{
    public const string ConfigPath = "config/esplus-common.toml";
    public const string DefaultUsername = "admin";
    public const string DefaultPasswordLiteral = "esplus";

    public static int ReadInt(string toml, string key, int fallback)
    {
        var m = Regex.Match(toml, $@"(?m)^\s*{Regex.Escape(key)}\s*=\s*(-?\d+)\s*$");
        return m.Success && int.TryParse(m.Groups[1].Value, out var n) ? n : fallback;
    }

    public static bool ReadBool(string toml, string key, bool fallback)
    {
        var m = Regex.Match(toml, $@"(?m)^\s*{Regex.Escape(key)}\s*=\s*(true|false)\s*$", RegexOptions.IgnoreCase);
        return m.Success ? bool.Parse(m.Groups[1].Value) : fallback;
    }

    public static string ReadString(string toml, string key, string fallback)
    {
        var quoted = Regex.Match(toml, $@"(?m)^\s*{Regex.Escape(key)}\s*=\s*""([^""]*)""\s*$");
        if (quoted.Success)
        {
            return quoted.Groups[1].Value;
        }

        var bare = Regex.Match(toml, $@"(?m)^\s*{Regex.Escape(key)}\s*=\s*(\S+)\s*$");
        return bare.Success ? bare.Groups[1].Value.Trim('\'') : fallback;
    }

    public static string Upsert(string toml, string key, string rendered)
    {
        var pattern = $@"(?m)^(\s*){Regex.Escape(key)}\s*=\s*.*$";
        if (Regex.IsMatch(toml, pattern))
        {
            return Regex.Replace(toml, pattern, "$1" + rendered);
        }

        var body = string.IsNullOrWhiteSpace(toml) ? "# Cocktail ESPlus adapter\n" : toml.TrimEnd();
        return body + "\n" + rendered + "\n";
    }

    public static string Quote(string value) =>
        "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";

    public static string ApplyPanel(
        string toml,
        int port,
        string bind,
        string username,
        string password,
        bool enabled)
    {
        var next = string.IsNullOrWhiteSpace(toml)
            ? """
              # Written by Cocktail ESPlus adapter. Server + client both need the ESPlus mod.
              # Panel bind stays loopback; do not set 0.0.0.0 unless you know the risk.

              """
            : toml;

        next = Upsert(next, "panelEnabled", $"panelEnabled = {(enabled ? "true" : "false")}");
        next = Upsert(next, "panelPort", $"panelPort = {port}");
        next = Upsert(next, "panelBindAddress", $"panelBindAddress = {Quote(bind)}");
        next = Upsert(next, "panelUsername", $"panelUsername = {Quote(username)}");
        next = Upsert(next, "panelPassword", $"panelPassword = {Quote(password)}");
        next = Upsert(next, "panelAllowDefaultPassword", "panelAllowDefaultPassword = false");
        return next;
    }
}
