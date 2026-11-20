use uuid::Uuid;
use zone_server::routes::audit::access::{self, FORBIDDEN, ORGANIZATION_MISSING};
use zone_server::routes::audit::refusal::Refusal;

use crate::common::audit::database;

const PREFIX: &str = "audit_access";

#[tokio::test]
async fn only_active_owners_and_admins_manage_an_organizations_audit_settings() {
    let database = database(PREFIX).await;
    let organization = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Audit', $1::text)")
        .bind(organization)
        .execute(&database.pool)
        .await
        .unwrap();
    let mut callers = Vec::new();
    for (role, active) in [
        ("owner", true),
        ("admin", true),
        ("member", true),
        ("admin", false),
    ] {
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $1::text, 'audit')")
            .bind(user)
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role, is_active) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(organization)
        .bind(user)
        .bind(role)
        .bind(active)
        .execute(&database.pool)
        .await
        .unwrap();
        callers.push(user);
    }
    let [owner, admin, member, departed] = callers[..] else {
        unreachable!("four callers were created");
    };

    let mut connection = database.pool.acquire().await.unwrap();
    for user in [owner, admin] {
        assert!(
            access::organization(&mut connection, organization, user)
                .await
                .is_ok()
        );
    }
    assert!(matches!(
        access::organization(&mut connection, organization, member).await,
        Err(Refusal::Forbidden(FORBIDDEN))
    ));
    for user in [departed, Uuid::new_v4()] {
        assert!(matches!(
            access::organization(&mut connection, organization, user).await,
            Err(Refusal::Missing(ORGANIZATION_MISSING))
        ));
    }
    assert!(matches!(
        access::organization(&mut connection, Uuid::new_v4(), owner).await,
        Err(Refusal::Missing(ORGANIZATION_MISSING))
    ));
    drop(connection);
    database.cleanup().await;
}
