namespace Cocktail.Sdk;

public interface IPluginContext
{
    PluginManifest Manifest { get; }
    IReadOnlySet<string> Permissions { get; }
    IPluginLog Log { get; }
    IPluginRouter Router { get; }
    IPluginEvents Events { get; }
    IPluginScheduler Scheduler { get; }
    ICocktailClient ControlPlane { get; }
    string DataDirectory { get; }
    bool Has(string permission);
}

public interface IPluginLog
{
    void Info(string message);
    void Warn(string message);
    void Error(string message, Exception? ex = null);
}

public interface IPluginScheduler
{
    void Every(TimeSpan period, Func<CancellationToken, Task> work);
}

public interface IPluginEvents
{
    void OnStatus(Func<InstanceStatusEvent, CancellationToken, Task> handler);
    void OnLog(Func<InstanceLogEvent, CancellationToken, Task> handler);
    void OnMetric(Func<InstanceMetricEvent, CancellationToken, Task> handler);
}

public sealed record InstanceStatusEvent(string InstanceId, string Status, DateTimeOffset At);

public sealed record InstanceLogEvent(string InstanceId, string Stream, string Line, DateTimeOffset At);

public sealed record InstanceMetricEvent(
    string InstanceId,
    DateTimeOffset At,
    float CpuPct,
    float MemoryMib,
    float? Tps,
    uint Players);
