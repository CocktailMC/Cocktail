using Cocktail.Sdk;

namespace Cocktail.Plugins.GameOps;

public sealed class GameOpsPlugin : ICocktailPlugin
{
    internal const string Example = """
        apiVersion: cocktail.gameops/v1
        kind: World
        metadata:
          name: survival
        spec:
          folder: world
          retainSnapshots: 5
        ---
        apiVersion: cocktail.gameops/v1
        kind: PluginSet
        metadata:
          name: survival-plugins
        spec:
          group: survival
          items:
            - name: spark
              enabled: true
              source: local
        ---
        apiVersion: cocktail.gameops/v1
        kind: Proxy
        metadata:
          name: hub
        spec:
          instanceName: velocity-hub
          group: survival
          listenPort: 25577
          createIfMissing: true
          desiredRunning: true
        ---
        apiVersion: cocktail.gameops/v1
        kind: Network
        metadata:
          name: smp
        spec:
          worlds: [survival]
          pluginSet: survival-plugins
          proxy: hub
          desiredRunning: true
          servers:
            - name: smp-1
              nodeId: local
              core: paper
              memoryMib: 2048
              port: 25565
              world: survival
              group: survival
        """;

    private ObjectStore? _store;
    private Reconciler? _reconciler;

    public Task StartAsync(IPluginContext context, CancellationToken cancellationToken)
    {
        _store = new ObjectStore(context.DataDirectory);
        _reconciler = new Reconciler(_store, context.ControlPlane, context.Log);
        context.Log.Info("GameOps kinds: World PluginSet Proxy Network (Node/Instance observed)");

        context.Router.Map("GET", "/summary", async _ =>
        {
            IReadOnlyList<JsonNode> nodes = [];
            IReadOnlyList<JsonInstance> instances = [];
            try { nodes = await context.ControlPlane.ListNodesAsync(); } catch { /* host may be up before login token */ }
            try { instances = await context.ControlPlane.ListInstancesAsync(); } catch { /* ignore */ }
            return PluginHttpResult.Json(new
            {
                apiVersion = GameOpsKinds.ApiVersion,
                owned = GameOpsKinds.Owned,
                observed = GameOpsKinds.Observed,
                objects = _store.List(),
                nodes,
                instances = instances.Select(i => new
                {
                    i.Id,
                    i.Status,
                    i.NodeId,
                    i.DesiredRunning,
                    i.Spec.Name,
                    i.Spec.Group,
                    i.Spec.Port,
                    i.Spec.Core,
                }),
            });
        });

        context.Router.Map("GET", "/example", _ =>
            Task.FromResult(PluginHttpResult.Text(Example)));

        context.Router.Map("GET", "/objects", _ =>
            Task.FromResult(PluginHttpResult.Json(new { items = _store.List() })));

        context.Router.Map("GET", "/objects/*", http =>
        {
            var parts = http.Path["/objects/".Length..].Split('/', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length == 1)
            {
                return Task.FromResult(PluginHttpResult.Json(new { items = _store.List(parts[0]) }));
            }

            if (parts.Length >= 2)
            {
                var obj = _store.Get(parts[0], parts[1]);
                return Task.FromResult(obj is null
                    ? PluginHttpResult.NotFound("object not found")
                    : PluginHttpResult.Json(obj));
            }

            return Task.FromResult(PluginHttpResult.BadRequest("kind/name required"));
        });

        context.Router.Map("PUT", "/objects/*", http =>
        {
            var parts = http.Path["/objects/".Length..].Split('/', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
            {
                return Task.FromResult(PluginHttpResult.BadRequest("PUT /objects/{kind}/{name}"));
            }

            var spec = http.Body.Length == 0
                ? SpecJson.Write(new { })
                : System.Text.Json.JsonDocument.Parse(http.Body).RootElement.Clone();
            if (spec.TryGetProperty("spec", out var inner))
            {
                spec = inner.Clone();
            }

            var obj = _store.Upsert(parts[0], parts[1], spec);
            return Task.FromResult(PluginHttpResult.Json(obj));
        });

        context.Router.Map("DELETE", "/objects/*", http =>
        {
            var parts = http.Path["/objects/".Length..].Split('/', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
            {
                return Task.FromResult(PluginHttpResult.BadRequest("DELETE /objects/{kind}/{name}"));
            }

            return Task.FromResult(_store.Delete(parts[0], parts[1])
                ? PluginHttpResult.Json(new { ok = true })
                : PluginHttpResult.NotFound("object not found"));
        });

        context.Router.Map("POST", "/apply", async http =>
        {
            try
            {
                var text = http.Text;
                if (http.Json<ApplyBody>() is { Yaml: { Length: > 0 } yaml })
                {
                    text = yaml;
                }

                var docs = ManifestParser.Parse(text);
                var stored = new List<StoredObject>();
                foreach (var doc in docs)
                {
                    stored.Add(_store.Upsert(doc.Kind, doc.Name, doc.Spec, doc.Labels));
                }

                await _reconciler.ReconcileAllAsync(CancellationToken.None);
                return PluginHttpResult.Json(new { ok = true, applied = stored.Count, items = stored });
            }
            catch (Exception ex)
            {
                return PluginHttpResult.BadRequest(ex.Message);
            }
        });

        context.Router.Map("POST", "/reconcile", async _ =>
        {
            await _reconciler.ReconcileAllAsync(CancellationToken.None);
            return PluginHttpResult.Json(new { ok = true, items = _store.List() });
        });

        context.Router.Map("POST", "/worlds/*", http =>
        {
            var parts = http.Path["/worlds/".Length..].Split('/', StringSplitOptions.RemoveEmptyEntries);
            var name = parts.ElementAtOrDefault(0);
            if (string.IsNullOrEmpty(name))
            {
                return Task.FromResult(PluginHttpResult.BadRequest("world name required"));
            }

            try
            {
                var dest = _reconciler.SnapshotWorld(name);
                return Task.FromResult(PluginHttpResult.Json(new { ok = true, snapshot = dest }));
            }
            catch (Exception ex)
            {
                return Task.FromResult(PluginHttpResult.BadRequest(ex.Message));
            }
        });

        context.Scheduler.Every(TimeSpan.FromSeconds(20), async ct =>
        {
            await _reconciler.ReconcileAllAsync(ct);
        });

        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private sealed class ApplyBody
    {
        [System.Text.Json.Serialization.JsonPropertyName("yaml")]
        public string? Yaml { get; set; }
    }
}
