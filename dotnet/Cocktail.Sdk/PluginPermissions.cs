namespace Cocktail.Sdk;

/// <summary>Capability flags declared in plugin.json. The host enforces them.</summary>
public static class PluginPermissions
{
    public const string HttpExpose = "http.expose";
    public const string EventsSubscribe = "events.subscribe";
    public const string EventsLogs = "events.logs";
    public const string Scheduler = "scheduler";
    public const string InstancesRead = "controlplane.instances.read";
    public const string InstancesWrite = "controlplane.instances.write";
    public const string NodesRead = "controlplane.nodes.read";
    public const string FilesWrite = "controlplane.files.write";
    public const string FilesRead = "controlplane.files.read";
    public const string UiContribute = "ui.contribute";
}
