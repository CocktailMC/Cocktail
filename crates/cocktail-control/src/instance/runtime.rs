//! A runtime handle is pinned to the init connection that created it.
use cocktail_shared::runtime::{INSTANCE_EVENT, RuntimeEvent};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub(crate) enum Connection {
    Remote(Arc<crate::init_client::InitClient>),
    #[cfg(test)]
    Local,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InitConnection")
    }
}

impl Connection {
    pub async fn current() -> anyhow::Result<Self> {
        if let Some(sup) = crate::SERVICE_SUPERVISOR.get() {
            return Ok(Self::Remote(sup.ipc_client(crate::INIT_SERVICE).await?));
        }
        #[cfg(test)]
        {
            Ok(Self::Local)
        }
        #[cfg(not(test))]
        {
            anyhow::bail!("cocktail-init is unavailable")
        }
    }

    pub async fn call<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> anyhow::Result<T> {
        let value = match self {
            Self::Remote(client) => client.call(method, params).await?,
            #[cfg(test)]
            Self::Local => cocktail_init::build_server()
                .call(method, serde_json::to_value(params)?)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e.message))?,
        };
        Ok(serde_json::from_value(value)?)
    }

    pub fn subscribe(&self) -> EventReceiver {
        match self {
            Self::Remote(client) => EventReceiver::Remote(client.subscribe_events()),
            #[cfg(test)]
            Self::Local => EventReceiver::Local(cocktail_init::registry::registry().subscribe()),
        }
    }

    pub async fn closed(&self) {
        match self {
            Self::Remote(client) => client.closed().await,
            #[cfg(test)]
            Self::Local => std::future::pending::<()>().await,
        }
    }
}

pub(crate) enum EventReceiver {
    Remote(broadcast::Receiver<cocktail_shared::proto::Event>),
    #[cfg(test)]
    Local(broadcast::Receiver<RuntimeEvent>),
}
impl EventReceiver {
    pub async fn recv(&mut self) -> Result<RuntimeEvent, broadcast::error::RecvError> {
        loop {
            match self {
                Self::Remote(rx) => {
                    let event = rx.recv().await?;
                    if event.method == INSTANCE_EVENT {
                        match serde_json::from_value(event.params) {
                            Ok(event) => return Ok(event),
                            Err(e) => tracing::warn!(error = %e, "invalid runtime event"),
                        }
                    }
                }
                #[cfg(test)]
                Self::Local(rx) => return rx.recv().await,
            }
        }
    }
}

pub(crate) async fn call<P: Serialize, T: DeserializeOwned>(
    method: &str,
    params: P,
) -> anyhow::Result<T> {
    Connection::current().await?.call(method, params).await
}
