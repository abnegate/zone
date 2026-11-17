use zone_enterprise::audit::{Event, VERSION};

use crate::db::audit::events::Row;

pub fn event(row: Row) -> Event {
    Event {
        id: row.id,
        version: VERSION,
        created_at: row.created_at,
        organization_id: row.organization_id,
        workspace_id: row.workspace_id,
        actor_id: row.actor_id,
        actor_email: row.actor_email,
        action: row.action,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        old_values: row.old_values,
        new_values: row.new_values,
        ip_address: row.ip_address,
        user_agent: row.user_agent,
    }
}
