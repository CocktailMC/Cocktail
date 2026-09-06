mod handlers;

use axum::routing::{any, delete, get, post, put};
use axum::Router;

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/v1/health", get(handlers::health))
        .route("/api/v1/setup", post(handlers::setup))
        .route("/api/v1/auth/login", post(handlers::login))
        .route("/api/v1/auth/logout", post(handlers::logout))
        .route("/api/v1/auth/me", get(handlers::me))
        .route("/api/v1/auth/password", axum::routing::put(handlers::change_password))
        .route(
            "/api/v1/settings",
            get(handlers::get_settings).put(handlers::update_settings),
        )
        .route("/api/v1/audit", get(handlers::list_audit))
        .route(
            "/api/v1/instances",
            get(handlers::list_instances).post(handlers::create_instance),
        )
        .route(
            "/api/v1/instances/{id}",
            get(handlers::get_instance)
                .put(handlers::update_instance)
                .delete(handlers::delete_instance),
        )
        .route("/api/v1/instances/{id}/start", post(handlers::start_instance))
        .route("/api/v1/instances/{id}/stop", post(handlers::stop_instance))
        .route(
            "/api/v1/instances/{id}/restart",
            post(handlers::restart_instance),
        )
        .route(
            "/api/v1/instances/{id}/command",
            post(handlers::send_command),
        )
        .route("/api/v1/instances/{id}/eula", post(handlers::accept_eula))
        .route("/api/v1/instances/{id}/logs", get(handlers::recent_logs))
        .route("/api/v1/instances/{id}/files", get(handlers::list_files))
        .route(
            "/api/v1/instances/{id}/files/content",
            get(handlers::read_file).put(handlers::write_file),
        )
        .route(
            "/api/v1/instances/{id}/files/content",
            delete(handlers::delete_file),
        )
        .route(
            "/api/v1/instances/{id}/files/download",
            get(handlers::download_file),
        )
        .route(
            "/api/v1/instances/{id}/files/upload",
            post(handlers::upload_file),
        )
        .route(
            "/api/v1/instances/{id}/files/mkdir",
            post(handlers::mkdir),
        )
        .route(
            "/api/v1/instances/{id}/install-jar",
            post(handlers::install_jar),
        )
        .route(
            "/api/v1/instances/{id}/startup-jar",
            post(handlers::set_startup_jar),
        )
        .route(
            "/api/v1/instances/{id}/backups",
            get(handlers::list_backups).post(handlers::create_backup),
        )
        .route(
            "/api/v1/instances/{id}/backups/{backup_id}",
            delete(handlers::delete_backup),
        )
        .route(
            "/api/v1/instances/{id}/backups/{backup_id}/restore",
            post(handlers::restore_backup),
        )
        .route(
            "/api/v1/instances/{id}/properties",
            get(handlers::get_properties).put(handlers::set_properties),
        )
        .route(
            "/api/v1/instances/{id}/plugins",
            get(handlers::list_plugins),
        )
        .route(
            "/api/v1/instances/{id}/plugins/{name}/enable",
            post(handlers::enable_plugin),
        )
        .route(
            "/api/v1/instances/{id}/plugins/{name}/disable",
            post(handlers::disable_plugin),
        )
        .route(
            "/api/v1/cores/{core}/versions",
            get(handlers::list_core_versions),
        )
        .route(
            "/api/v1/instances/{id}/install",
            post(handlers::install_core),
        )
        .route("/api/v1/modrinth/search", get(handlers::modrinth_search))
        .route(
            "/api/v1/modrinth/projects/{id}/versions",
            get(handlers::modrinth_versions),
        )
        .route(
            "/api/v1/instances/{id}/modrinth/install",
            post(handlers::modrinth_install),
        )
        .route("/api/v1/hangar/search", get(handlers::hangar_search))
        .route(
            "/api/v1/hangar/projects/{slug}/versions",
            get(handlers::hangar_versions),
        )
        .route(
            "/api/v1/instances/{id}/hangar/install",
            post(handlers::hangar_install),
        )
        .route("/api/v1/spiget/search", get(handlers::spiget_search))
        .route(
            "/api/v1/spiget/resources/{rid}/versions",
            get(handlers::spiget_versions),
        )
        .route(
            "/api/v1/spiget/resources/{rid}/icon",
            get(handlers::spiget_icon),
        )
        .route(
            "/api/v1/instances/{id}/spiget/install",
            post(handlers::spiget_install),
        )
        .route(
            "/api/v1/instances/{id}/players",
            get(handlers::list_players),
        )
        .route(
            "/api/v1/instances/{id}/players/{name}/{action}",
            post(handlers::player_action),
        )
        .route(
            "/api/v1/instances/{id}/worlds",
            get(handlers::list_worlds),
        )
        .route(
            "/api/v1/instances/{id}/worlds/{world}/reset",
            post(handlers::reset_world),
        )
        .route(
            "/api/v1/instances/{id}/worlds/{world}/export",
            post(handlers::export_world),
        )
        .route(
            "/api/v1/instances/{id}/worlds/{world}/import",
            post(handlers::import_world),
        )
        .route(
            "/api/v1/schedules",
            get(handlers::list_schedules).post(handlers::create_schedule),
        )
        .route("/api/v1/schedules/{id}", delete(handlers::delete_schedule))
        .route("/api/v1/fleet/summary", get(handlers::fleet_summary))
        .route("/api/v1/fleet/bulk", post(handlers::fleet_bulk))
        .route("/api/v1/docker/status", get(handlers::docker_status))
        .route("/api/v1/events/ws", get(handlers::events_ws))
        .route("/api/v1/instances/{id}/logs/ws", get(handlers::logs_ws))
        .route("/api/v1/agent/ws", get(crate::cluster::agent_ws))
        .route(
            "/api/v1/nodes",
            get(handlers::list_nodes).post(handlers::create_node),
        )
        .route("/api/v1/nodes/{id}", delete(handlers::delete_node))
        .route(
            "/api/v1/instances/{id}/spec",
            get(handlers::get_instance_spec).put(handlers::apply_instance_spec),
        )
        .route("/api/v1/extensions", get(handlers::list_extensions))
        .route("/api/v1/extensions/reload", post(handlers::reload_extensions))
        .route(
            "/api/v1/extensions/{id}",
            put(handlers::set_extension_enabled),
        )
        .route("/api/v1/ext/{plugin_id}", any(handlers::proxy_extension_root))
        .route(
            "/api/v1/ext/{plugin_id}/{*rest}",
            any(handlers::proxy_extension),
        )
}
