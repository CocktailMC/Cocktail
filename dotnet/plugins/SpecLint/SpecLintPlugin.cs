using Cocktail.Sdk;

namespace Cocktail.Plugins.SpecLint;

public sealed class SpecLintPlugin : ICocktailPlugin
{
    public Task StartAsync(IPluginContext context, CancellationToken cancellationToken)
    {
        context.Log.Info("spec lint plugin online");
        context.Router.Map("GET", "/report", async _ =>
        {
            var instances = await context.ControlPlane.ListInstancesAsync();
            var findings = new List<object>();
            var usedPorts = new Dictionary<int, string>();
            foreach (var inst in instances)
            {
                var spec = inst.Spec;
                var node = inst.NodeId ?? spec.NodeId ?? "local";
                if (spec.Core is not "demo" && !spec.EulaAccepted)
                {
                    findings.Add(Finding(inst, "eula", "error", "未同意 EULA，无法作为可玩状态上线"));
                }

                if (spec.DesiredRunning || inst.DesiredRunning)
                {
                    if (inst.Status is "stopped" or "created" or "crashed")
                    {
                        findings.Add(Finding(inst, "drift", "warn", $"期望运行但现状为 {inst.Status}"));
                    }
                }

                if (spec.Port is < 1 or > 65535)
                {
                    findings.Add(Finding(inst, "port", "error", $"非法端口 {spec.Port}"));
                }
                else if (usedPorts.TryGetValue(spec.Port, out var other))
                {
                    findings.Add(Finding(inst, "port", "warn", $"端口 {spec.Port} 与 {other} 冲突（同清单）"));
                }
                else
                {
                    usedPorts[spec.Port] = spec.Name;
                }

                if (string.IsNullOrWhiteSpace(node))
                {
                    findings.Add(Finding(inst, "node", "warn", "未指定 node_id"));
                }
            }

            return PluginHttpResult.Json(new
            {
                plugin = context.Manifest.Id,
                generatedAt = DateTimeOffset.UtcNow,
                instances = instances.Count,
                findings,
            });
        });
        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private static object Finding(JsonInstance inst, string code, string severity, string message) => new
    {
        instanceId = inst.Id,
        name = inst.Spec.Name,
        code,
        severity,
        message,
        generation = inst.Generation,
    };
}
