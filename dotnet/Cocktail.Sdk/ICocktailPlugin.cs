namespace Cocktail.Sdk;

public interface ICocktailPlugin
{
    Task StartAsync(IPluginContext context, CancellationToken cancellationToken);
    Task StopAsync(CancellationToken cancellationToken);
}
