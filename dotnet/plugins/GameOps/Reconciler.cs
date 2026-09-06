using Cocktail.Sdk;

namespace Cocktail.Plugins.GameOps;

internal sealed class Reconciler(ObjectStore store, ICocktailClient plane, IPluginLog log)
{
    public async Task ReconcileAllAsync(CancellationToken ct)
    {
        foreach (var obj in store.List())
        {
            try
            {
                await ReconcileOneAsync(obj, ct);
            }
            catch (Exception ex)
            {
                log.Error($"reconcile {obj.Kind}/{obj.Name} failed", ex);
                store.PutStatus(obj.Kind, obj.Name, new { ready = false, message = ex.Message });
            }
        }
    }

    public Task ReconcileOneAsync(StoredObject obj, CancellationToken ct) =>
        obj.Kind switch
        {
            GameOpsKinds.World => ReconcileWorld(obj, ct),
            GameOpsKinds.PluginSet => ReconcilePluginSet(obj, ct),
            GameOpsKinds.Proxy => ReconcileProxy(obj, ct),
            GameOpsKinds.Network => ReconcileNetwork(obj, ct),
            _ => Task.CompletedTask,
        };

    private Task ReconcileWorld(StoredObject obj, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();
        var spec = SpecJson.Read<WorldSpec>(obj.Spec);
        var path = WorldPath(obj.Name, spec);
        Directory.CreateDirectory(path);
        if (!string.IsNullOrWhiteSpace(spec.CloneFrom))
        {
            var src = Directory.Exists(spec.CloneFrom)
                ? spec.CloneFrom
                : WorldPath(spec.CloneFrom, store.Get(GameOpsKinds.World, spec.CloneFrom) is { } other
                    ? SpecJson.Read<WorldSpec>(other.Spec)
                    : new WorldSpec());
            if (Directory.Exists(src) && DirectoryEmpty(path))
            {
                CopyDirectory(src, path);
            }
        }

        TouchLevelDat(path);
        var snaps = Directory.Exists(SnapshotDir(path))
            ? Directory.GetDirectories(SnapshotDir(path)).Length
            : 0;
        store.PutStatus(obj.Kind, obj.Name, new
        {
            ready = true,
            path,
            bytes = DirSize(path),
            snapshots = snaps,
            message = "world directory ready",
        });
        return Task.CompletedTask;
    }

    private async Task ReconcilePluginSet(StoredObject obj, CancellationToken ct)
    {
        var spec = SpecJson.Read<PluginSetSpec>(obj.Spec);
        var instances = await SelectInstances(spec, ct);
        var drift = new List<object>();
        foreach (var inst in instances)
        {
            IReadOnlyList<JsonPlugin> have;
            try
            {
                have = await plane.ListPluginsAsync(inst.Id, ct);
            }
            catch (Exception ex)
            {
                drift.Add(new { instanceId = inst.Id, error = ex.Message });
                continue;
            }

            foreach (var item in spec.Items)
            {
                var match = have.FirstOrDefault(p => NamesEqual(p.Name, item.Name));
                if (match is null)
                {
                    if (item.Source.Equals("modrinth", StringComparison.OrdinalIgnoreCase)
                        && !string.IsNullOrWhiteSpace(item.Project))
                    {
                        try
                        {
                            await plane.InstallModrinthAsync(inst.Id, item.Project, item.Version, ct);
                            drift.Add(new { instanceId = inst.Id, plugin = item.Name, action = "installed" });
                        }
                        catch (Exception ex)
                        {
                            drift.Add(new { instanceId = inst.Id, plugin = item.Name, want = "present", have = "missing", error = ex.Message });
                        }
                    }
                    else
                    {
                        drift.Add(new { instanceId = inst.Id, plugin = item.Name, want = "present", have = "missing" });
                    }

                    continue;
                }

                var jarEnabled = match.Enabled;
                if (jarEnabled != item.Enabled)
                {
                    try
                    {
                        await plane.EnablePluginAsync(inst.Id, match.Name, item.Enabled, ct);
                        drift.Add(new { instanceId = inst.Id, plugin = item.Name, action = item.Enabled ? "enabled" : "disabled" });
                    }
                    catch (Exception ex)
                    {
                        drift.Add(new { instanceId = inst.Id, plugin = item.Name, error = ex.Message });
                    }
                }
            }
        }

        store.PutStatus(obj.Kind, obj.Name, new
        {
            ready = drift.All(d => d.ToString()?.Contains("error") != true),
            targets = instances.Select(i => i.Spec.Name).ToArray(),
            drift,
            message = drift.Count == 0 ? "in sync" : "drift recorded",
        });
    }

    private async Task ReconcileProxy(StoredObject obj, CancellationToken ct)
    {
        var spec = SpecJson.Read<ProxySpec>(obj.Spec);
        var instances = (await plane.ListInstancesAsync(ct)).ToList();
        var name = spec.InstanceName ?? obj.Name;
        var proxyInst = instances.FirstOrDefault(i =>
            i.Spec.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
        if (proxyInst is null && spec.CreateIfMissing)
        {
            proxyInst = await plane.CreateInstanceAsync(new CreateInstanceBody
            {
                Name = name,
                Core = "custom",
                MemoryMib = 512,
                Port = spec.ListenPort,
                NodeId = spec.NodeId ?? "local",
                Group = spec.Group ?? "proxy",
                Tags = ["proxy", "gameops"],
                EulaAccepted = true,
            }, ct);
            instances.Add(proxyInst);
        }

        if (proxyInst is null)
        {
            store.PutStatus(obj.Kind, obj.Name, new { ready = false, message = $"proxy instance {name} missing" });
            return;
        }

        var backends = instances
            .Where(i => i.Id != proxyInst.Id)
            .Where(i => string.IsNullOrEmpty(spec.Group)
                        || string.Equals(i.Spec.Group, spec.Group, StringComparison.OrdinalIgnoreCase))
            .Select(i => new
            {
                name = i.Spec.Name,
                address = $"{i.Spec.Name}:{i.Spec.Port}",
                healthy = i.Status is "running" or "starting",
                instanceId = i.Id,
            })
            .ToList();

        var toml = BuildVelocityToml(spec, backends.Select(b => (b.name, $"127.0.0.1:{instances.First(i => i.Id == b.instanceId).Spec.Port}")));
        try
        {
            await plane.WriteFileAsync(proxyInst.Id, "velocity.toml", toml, ct);
        }
        catch (Exception ex)
        {
            log.Warn($"proxy config write skipped: {ex.Message}");
        }

        if (spec.DesiredRunning && proxyInst.Status is "stopped" or "created" or "crashed")
        {
            try
            {
                await plane.StartInstanceAsync(proxyInst.Id, ct);
            }
            catch (Exception ex)
            {
                log.Warn($"proxy start skipped: {ex.Message}");
            }
        }

        store.PutStatus(obj.Kind, obj.Name, new
        {
            ready = true,
            instanceId = proxyInst.Id,
            backends,
            message = "proxy backends synced",
        });
    }

    private async Task ReconcileNetwork(StoredObject obj, CancellationToken ct)
    {
        var spec = SpecJson.Read<NetworkSpec>(obj.Spec);
        foreach (var worldName in spec.Worlds)
        {
            if (store.Get(GameOpsKinds.World, worldName) is { } world)
            {
                await ReconcileWorld(world, ct);
            }
        }

        var instances = (await plane.ListInstancesAsync(ct)).ToList();
        var ids = new List<string>();
        foreach (var server in spec.Servers)
        {
            var inst = instances.FirstOrDefault(i =>
                i.Spec.Name.Equals(server.Name, StringComparison.OrdinalIgnoreCase));
            if (inst is null)
            {
                inst = await plane.CreateInstanceAsync(new CreateInstanceBody
                {
                    Name = server.Name,
                    Core = server.Core,
                    MemoryMib = server.MemoryMib,
                    Port = server.Port,
                    NodeId = server.NodeId ?? "local",
                    Group = server.Group,
                    Tags = ["gameops", obj.Name],
                    EulaAccepted = true,
                    AutoRestart = true,
                }, ct);
                instances.Add(inst);
            }

            ids.Add(inst.Id);
            if (!string.IsNullOrWhiteSpace(server.World)
                && store.Get(GameOpsKinds.World, server.World) is { } worldObj)
            {
                AttachWorld(inst, worldObj);
            }

            if (spec.DesiredRunning && inst.Status is "stopped" or "created")
            {
                try
                {
                    await plane.StartInstanceAsync(inst.Id, ct);
                }
                catch (Exception ex)
                {
                    log.Warn($"network start {server.Name}: {ex.Message}");
                }
            }
        }

        if (!string.IsNullOrWhiteSpace(spec.PluginSet)
            && store.Get(GameOpsKinds.PluginSet, spec.PluginSet) is { } set)
        {
            await ReconcilePluginSet(set, ct);
        }

        if (!string.IsNullOrWhiteSpace(spec.Proxy)
            && store.Get(GameOpsKinds.Proxy, spec.Proxy) is { } proxy)
        {
            await ReconcileProxy(proxy, ct);
        }

        store.PutStatus(obj.Kind, obj.Name, new
        {
            ready = true,
            instanceIds = ids,
            worlds = spec.Worlds,
            pluginSet = spec.PluginSet,
            proxy = spec.Proxy,
            message = "network reconciled",
        });
    }

    private void AttachWorld(JsonInstance inst, StoredObject world)
    {
        var spec = SpecJson.Read<WorldSpec>(world.Spec);
        var src = WorldPath(world.Name, spec);
        if (string.IsNullOrWhiteSpace(inst.Spec.Workdir) || !Directory.Exists(src))
        {
            return;
        }

        var dest = Path.Combine(inst.Spec.Workdir, spec.Folder);
        try
        {
            if (Directory.Exists(dest) || File.Exists(dest))
            {
                return;
            }

            Directory.CreateDirectory(inst.Spec.Workdir);
            Directory.CreateSymbolicLink(dest, Path.GetFullPath(src));
        }
        catch (Exception ex)
        {
            log.Warn($"world attach {world.Name} -> {inst.Spec.Name}: {ex.Message}");
        }
    }

    private async Task<List<JsonInstance>> SelectInstances(PluginSetSpec spec, CancellationToken ct)
    {
        var all = await plane.ListInstancesAsync(ct);
        return all.Where(i =>
        {
            if (spec.InstanceNames.Count > 0)
            {
                return spec.InstanceNames.Contains(i.Spec.Name, StringComparer.OrdinalIgnoreCase);
            }

            if (!string.IsNullOrWhiteSpace(spec.Group))
            {
                return string.Equals(i.Spec.Group, spec.Group, StringComparison.OrdinalIgnoreCase);
            }

            return false;
        }).ToList();
    }

    public string SnapshotWorld(string name)
    {
        var obj = store.Get(GameOpsKinds.World, name)
                  ?? throw new InvalidOperationException("world not found");
        var spec = SpecJson.Read<WorldSpec>(obj.Spec);
        var path = WorldPath(name, spec);
        var snapRoot = SnapshotDir(path);
        Directory.CreateDirectory(snapRoot);
        var dest = Path.Combine(snapRoot, DateTime.UtcNow.ToString("yyyyMMdd-HHmmss"));
        CopyDirectory(path, dest);
        var keep = Math.Max(1, spec.RetainSnapshots);
        var extras = Directory.GetDirectories(snapRoot).OrderByDescending(d => d).Skip(keep);
        foreach (var extra in extras)
        {
            Directory.Delete(extra, recursive: true);
        }

        return dest;
    }

    private static string WorldPath(string name, WorldSpec spec)
    {
        if (!string.IsNullOrWhiteSpace(spec.Path))
        {
            return Path.GetFullPath(spec.Path);
        }

        return Path.GetFullPath(Path.Combine("data", "gameops", "worlds", name));
    }

    private static string SnapshotDir(string worldPath) => Path.Combine(worldPath, ".snapshots");

    private static bool DirectoryEmpty(string path) =>
        !Directory.Exists(path) || !Directory.EnumerateFileSystemEntries(path).Any(p => Path.GetFileName(p) != ".snapshots");

    private static void CopyDirectory(string src, string dest)
    {
        Directory.CreateDirectory(dest);
        foreach (var dir in Directory.GetDirectories(src, "*", SearchOption.AllDirectories))
        {
            if (dir.Contains($"{Path.DirectorySeparatorChar}.snapshots", StringComparison.Ordinal))
            {
                continue;
            }

            Directory.CreateDirectory(dir.Replace(src, dest));
        }

        foreach (var file in Directory.GetFiles(src, "*", SearchOption.AllDirectories))
        {
            if (file.Contains($"{Path.DirectorySeparatorChar}.snapshots", StringComparison.Ordinal))
            {
                continue;
            }

            var target = file.Replace(src, dest);
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            File.Copy(file, target, overwrite: true);
        }
    }

    private static long DirSize(string path)
    {
        if (!Directory.Exists(path))
        {
            return 0;
        }

        return Directory.GetFiles(path, "*", SearchOption.AllDirectories).Sum(f => new FileInfo(f).Length);
    }

    private static void TouchLevelDat(string path)
    {
        var marker = Path.Combine(path, "level.dat");
        if (!File.Exists(marker))
        {
            File.WriteAllText(Path.Combine(path, ".cocktail-world"), $"gameops world {DateTimeOffset.UtcNow:o}");
        }
    }

    private static bool NamesEqual(string have, string want)
    {
        var a = Path.GetFileNameWithoutExtension(have);
        var b = Path.GetFileNameWithoutExtension(want);
        return a.Equals(b, StringComparison.OrdinalIgnoreCase)
               || have.Equals(want, StringComparison.OrdinalIgnoreCase);
    }

    private static string BuildVelocityToml(ProxySpec spec, IEnumerable<(string name, string address)> backends)
    {
        var sb = new System.Text.StringBuilder();
        sb.AppendLine($"bind = \"0.0.0.0:{spec.ListenPort}\"");
        sb.AppendLine($"motd = \"{spec.Motd.Replace("\"", "\\\"")}\"");
        sb.AppendLine("[servers]");
        foreach (var (name, address) in backends)
        {
            sb.AppendLine($"{Sanitize(name)} = \"{address}\"");
        }

        sb.AppendLine("[forced-hosts]");
        return sb.ToString();
    }

    private static string Sanitize(string name) =>
        new string(name.Select(ch => char.IsLetterOrDigit(ch) ? ch : '_').ToArray());
}
