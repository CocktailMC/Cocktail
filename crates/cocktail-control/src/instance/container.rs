//! Container API delegates all engine access to init.
use super::{
    process::{self, ProcessHandle},
    runtime,
};
pub use cocktail_shared::runtime::{DockerImage, DockerStatus};
use cocktail_shared::{
    model::InstanceEvent,
    runtime::{LaunchKind, NameRequest},
};
use tokio::sync::broadcast;

pub async fn docker_status() -> DockerStatus {
    runtime::call("container.status", ())
        .await
        .unwrap_or_else(|e| DockerStatus {
            available: false,
            version: None,
            message: e.to_string(),
        })
}
pub async fn list_images() -> anyhow::Result<Vec<DockerImage>> {
    runtime::call("container.images", ()).await
}
pub async fn pull_image(image: &str) -> anyhow::Result<()> {
    runtime::call("container.pull", NameRequest { name: image.into() }).await
}
pub async fn spawn_docker_instance(
    instance_id: String,
    workdir: String,
    command: Option<String>,
    args: Vec<String>,
    memory_mib: u32,
    port: u16,
    cpu_limit: Option<f32>,
    image: &str,
    events: broadcast::Sender<InstanceEvent>,
) -> anyhow::Result<ProcessHandle> {
    process::launch(
        instance_id,
        workdir,
        port,
        LaunchKind::Container {
            command,
            args,
            memory_mib,
            cpu_limit,
            image: image.into(),
        },
        events,
    )
    .await
}
