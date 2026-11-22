use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::fleet::eligibility::shortfall;
use crate::db::fleet::labels;
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::mode::Mode;
use crate::db::fleet::policy;
use crate::db::fleet::runners::{self, Change, Registered, Removal, Runner};
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;
use crate::db::fleet::transition::Transition;
use crate::db::testing::Database;

mod claim;

struct Fleet {
    database: Database,
}

impl Fleet {
    async fn new() -> Self {
        Self {
            database: Database::create("fleet").await,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn register(
        &self,
        name: &str,
        instance: Uuid,
        version: &str,
        carried: &str,
    ) -> Registered {
        runners::upsert(
            self.pool(),
            name,
            instance,
            &Version::parse(version).unwrap(),
            &Labels::parse_list(carried).unwrap(),
        )
        .await
        .expect("a registration")
    }

    async fn runner(&self, name: &str, version: &str, carried: &str) -> Runner {
        self.register(name, Uuid::new_v4(), version, carried)
            .await
            .runner
    }

    async fn age(&self, runner: Uuid, seconds: f64) {
        sqlx::query(
            "UPDATE fleet_runners SET last_seen_at = now() - make_interval(secs => $2) WHERE id = $1",
        )
        .bind(runner)
        .bind(seconds)
        .execute(self.pool())
        .await
        .expect("an aged runner");
    }

    async fn set(&self, runner: Uuid, state: State) {
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(runner)
            .bind(state.as_str())
            .execute(self.pool())
            .await
            .expect("a runner state");
    }

    async fn user(&self) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) \
             VALUES (gen_random_uuid()::text || '@zone.test', 'test-only') RETURNING id",
        )
        .fetch_one(self.pool())
        .await
        .expect("a user")
    }

    async fn workspace(&self) -> (Uuid, Uuid) {
        let organization: Uuid = sqlx::query_scalar(
            "INSERT INTO organizations (name, slug) VALUES ('Fleet', gen_random_uuid()::text) \
             RETURNING id",
        )
        .fetch_one(self.pool())
        .await
        .expect("an organization");
        let workspace: Uuid = sqlx::query_scalar(
            "INSERT INTO workspaces (organization_id, name, slug) \
             VALUES ($1, 'Fleet', gen_random_uuid()::text) RETURNING id",
        )
        .bind(organization)
        .fetch_one(self.pool())
        .await
        .expect("a workspace");
        (organization, workspace)
    }

    async fn minimum(&self, minimum: Option<&str>) {
        let actor = self.user().await;
        let minimum = minimum.map(|minimum| Minimum::parse(minimum).unwrap());
        let mut transaction = self.pool().begin().await.unwrap();
        policy::save(&mut transaction, minimum.as_ref(), actor)
            .await
            .expect("a saved policy");
        transaction.commit().await.unwrap();
    }

    async fn require(&self, organization: Option<Uuid>, workspace: Option<Uuid>, required: &str) {
        let actor = self.user().await;
        let required = Labels::parse_list(required).unwrap();
        let mut transaction = self.pool().begin().await.unwrap();
        if let Some(organization) = organization {
            labels::save_organization(&mut transaction, organization, &required, actor)
                .await
                .expect("organization labels");
        }
        if let Some(workspace) = workspace {
            labels::save_workspace(&mut transaction, workspace, &required, actor)
                .await
                .expect("workspace labels");
        }
        transaction.commit().await.unwrap();
    }

    async fn live_run(&self, workspace: Uuid, runner: Uuid) {
        let task: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description) VALUES ($1, 'Run', 'Run') \
             RETURNING id",
        )
        .bind(workspace)
        .fetch_one(self.pool())
        .await
        .expect("a task");
        sqlx::query(
            "INSERT INTO task_runs (task_id, status, runner_id) VALUES ($1, 'running', $2)",
        )
        .bind(task)
        .bind(runner)
        .execute(self.pool())
        .await
        .expect("a live run");
    }

    async fn live_lease(&self, workspace: Uuid, runner: Uuid) {
        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name) VALUES ($1, 'Chat', 'model') \
             RETURNING id",
        )
        .bind(workspace)
        .fetch_one(self.pool())
        .await
        .expect("a chat");
        sqlx::query(
            "INSERT INTO chat_leases (chat_id, owner, fence, expires_at, runner_id) \
             VALUES ($1, gen_random_uuid(), 1, now() + interval '30 seconds', $2)",
        )
        .bind(chat)
        .bind(runner)
        .execute(self.pool())
        .await
        .expect("a live lease");
    }

    async fn remaining(&self, change: Change) -> i64 {
        let mut transaction = self.pool().begin().await.unwrap();
        policy::lock(&mut transaction).await.unwrap();
        let remaining = runners::remaining(&mut transaction, change)
            .await
            .expect("the remaining runners");
        transaction.rollback().await.unwrap();
        remaining
    }

    async fn remove(self) {
        self.database.remove().await;
    }
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn set(text: &str) -> Labels {
    Labels::parse_list(text).unwrap()
}

#[tokio::test]
async fn a_name_registers_once_and_keeps_its_state_across_processes() {
    let fleet = Fleet::new().await;
    let first = Uuid::new_v4();
    let created = fleet.register("eu-1", first, "1.4.0", "gpu").await;
    assert!(created.created);
    assert!(!created.contested);
    assert_eq!(created.runner.state, State::Active);
    assert_eq!(created.runner.instance, first);
    assert_eq!(created.runner.labels, set("gpu"));
    fleet.set(created.runner.id, State::Cordoned).await;

    let refreshed = fleet
        .register("eu-1", first, "1.4.1", "gpu,region=eu")
        .await;
    assert!(!refreshed.created);
    assert!(!refreshed.contested);
    assert_eq!(refreshed.runner.id, created.runner.id);
    assert_eq!(refreshed.runner.state, State::Cordoned);
    assert_eq!(refreshed.runner.version, version("1.4.1"));
    assert_eq!(refreshed.runner.labels, set("gpu,region=eu"));
    assert_eq!(refreshed.runner.started_at, created.runner.started_at);
    assert!(refreshed.runner.last_seen_at >= created.runner.last_seen_at);

    let rival = Uuid::new_v4();
    let contested = fleet.register("eu-1", rival, "1.4.1", "gpu").await;
    assert!(!contested.created);
    assert!(contested.contested);
    assert_eq!(contested.runner.instance, rival);
    assert_eq!(contested.runner.state, State::Cordoned);
    assert!(contested.runner.started_at > created.runner.started_at);

    fleet.age(created.runner.id, 61.0).await;
    let restarted = fleet.register("eu-1", Uuid::new_v4(), "1.4.1", "gpu").await;
    assert!(!restarted.contested);
    assert_eq!(restarted.runner.id, created.runner.id);

    let other = fleet.register("eu-2", Uuid::new_v4(), "1.4.1", "").await;
    assert!(other.created);
    assert_ne!(other.runner.id, created.runner.id);
    fleet.remove().await;
}

#[tokio::test]
async fn runners_are_listed_by_name_with_their_live_runs() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    let busy = fleet.runner("b", "1.4.0", "").await;
    let idle = fleet.runner("a", "1.4.0", "").await;
    fleet.live_run(workspace, busy.id).await;

    let listed = runners::list(fleet.pool()).await.unwrap();
    let names: Vec<(&str, i64)> = listed
        .iter()
        .map(|listed| (listed.runner.name.as_str(), listed.runs))
        .collect();
    assert_eq!(names, [("a", 0), ("b", 1)]);
    assert_eq!(
        runners::get(fleet.pool(), idle.id).await.unwrap(),
        Some(idle)
    );
    assert_eq!(
        runners::get(fleet.pool(), Uuid::new_v4()).await.unwrap(),
        None
    );
    fleet.remove().await;
}

#[tokio::test]
async fn forgetting_keeps_revoked_online_and_busy_runners() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    let gone = fleet.runner("gone", "1.4.0", "").await;
    let revoked = fleet.runner("revoked", "1.4.0", "").await;
    let online = fleet.runner("online", "1.4.0", "").await;
    let running = fleet.runner("running", "1.4.0", "").await;
    let chatting = fleet.runner("chatting", "1.4.0", "").await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.live_run(workspace, running.id).await;
    fleet.live_lease(workspace, chatting.id).await;
    for runner in [&gone, &revoked, &running, &chatting] {
        fleet.age(runner.id, 8.0 * 24.0 * 3600.0).await;
    }
    fleet.age(online.id, 5.0).await;

    let forgotten = runners::forget(fleet.pool(), chrono::TimeDelta::days(7))
        .await
        .unwrap();
    assert_eq!(
        forgotten.iter().map(|runner| runner.id).collect::<Vec<_>>(),
        [gone.id]
    );
    let kept: Vec<String> = runners::list(fleet.pool())
        .await
        .unwrap()
        .into_iter()
        .map(|listed| listed.runner.name)
        .collect();
    assert_eq!(kept, ["chatting", "online", "revoked", "running"]);
    fleet.remove().await;
}

#[tokio::test]
async fn only_an_offline_runner_can_be_removed() {
    let fleet = Fleet::new().await;
    let runner = fleet.runner("eu-1", "1.4.0", "").await;
    assert_eq!(
        runners::remove(fleet.pool(), runner.id).await.unwrap(),
        Removal::Online
    );
    fleet.age(runner.id, 61.0).await;
    let Removal::Removed(removed) = runners::remove(fleet.pool(), runner.id).await.unwrap() else {
        panic!("an offline runner is removed");
    };
    assert_eq!(removed.id, runner.id);
    assert_eq!(
        runners::remove(fleet.pool(), runner.id).await.unwrap(),
        Removal::Missing
    );
    fleet.remove().await;
}

#[tokio::test]
async fn a_state_change_writes_only_when_it_changes_something() {
    let fleet = Fleet::new().await;
    let runner = fleet.runner("eu-1", "1.4.0", "").await;
    let actor = fleet.user().await;
    let mut transaction = fleet.pool().begin().await.unwrap();
    let cordoned = runners::set_state(&mut transaction, runner.id, Transition::Cordon, actor)
        .await
        .unwrap()
        .expect("a known runner");
    assert!(cordoned.changed());
    assert_eq!(cordoned.previous, State::Active);
    assert_eq!(cordoned.runner.state, State::Cordoned);
    assert_eq!(cordoned.runner.state_changed_by, Some(actor));
    assert!(cordoned.runner.state_changed_at.is_some());

    let again = runners::set_state(&mut transaction, runner.id, Transition::Cordon, actor)
        .await
        .unwrap()
        .unwrap();
    assert!(!again.changed());
    assert_eq!(again.runner, cordoned.runner);

    let missing = runners::set_state(
        &mut transaction,
        Uuid::new_v4(),
        Transition::Activate,
        actor,
    )
    .await
    .unwrap();
    assert_eq!(missing, None);
    transaction.commit().await.unwrap();

    fleet.set(runner.id, State::Revoked).await;
    let mut transaction = fleet.pool().begin().await.unwrap();
    let activated = runners::set_state(&mut transaction, runner.id, Transition::Activate, actor)
        .await
        .unwrap()
        .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(activated.previous, State::Revoked);
    assert_eq!(activated.runner.state, State::Active);
    fleet.remove().await;
}

#[tokio::test]
async fn remaining_counts_only_connected_active_current_runners() {
    let fleet = Fleet::new().await;
    let current = fleet.runner("current", "1.5.0", "").await;
    let older = fleet.runner("older", "1.4.0", "").await;
    let cordoned = fleet.runner("cordoned", "1.5.0", "").await;
    let revoked = fleet.runner("revoked", "1.5.0", "").await;
    let offline = fleet.runner("offline", "1.5.0", "").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.age(offline.id, 61.0).await;

    assert_eq!(fleet.remaining(Change::Minimum(None)).await, 2);
    assert_eq!(fleet.remaining(Change::Cordon(current.id)).await, 1);
    assert_eq!(fleet.remaining(Change::Revoke(older.id)).await, 1);
    assert_eq!(fleet.remaining(Change::Cordon(cordoned.id)).await, 2);
    let raised = Minimum::parse("1.5.0").unwrap();
    assert_eq!(fleet.remaining(Change::Minimum(Some(raised))).await, 1);
    let above = Minimum::parse("2.0.0").unwrap();
    assert_eq!(fleet.remaining(Change::Minimum(Some(above))).await, 0);

    fleet.minimum(Some("1.5.0")).await;
    assert_eq!(fleet.remaining(Change::Cordon(older.id)).await, 1);
    assert_eq!(fleet.remaining(Change::Revoke(current.id)).await, 0);
    assert_eq!(fleet.remaining(Change::Minimum(None)).await, 2);
    fleet.remove().await;
}

#[tokio::test]
async fn matching_counts_connected_active_current_runners_with_the_labels() {
    let fleet = Fleet::new().await;
    fleet.runner("current", "1.5.0", "gpu,region=eu").await;
    fleet.runner("plain", "1.5.0", "").await;
    fleet.runner("older", "1.4.0", "gpu,region=eu").await;
    let cordoned = fleet.runner("cordoned", "1.5.0", "gpu,region=eu").await;
    let revoked = fleet.runner("revoked", "1.5.0", "gpu,region=eu").await;
    let offline = fleet.runner("offline", "1.5.0", "gpu,region=eu").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.age(offline.id, 61.0).await;
    fleet.minimum(Some("1.5.0")).await;

    let matching = |required: &'static str| {
        let pool = fleet.pool().clone();
        async move { labels::matching(&pool, &set(required)).await.unwrap() }
    };
    assert_eq!(matching("gpu,region=eu").await, 1);
    assert_eq!(matching("gpu").await, 1);
    assert_eq!(matching("").await, 2);
    assert_eq!(matching("region=us").await, 0);

    fleet.minimum(None).await;
    assert_eq!(matching("gpu").await, 2);
    fleet.remove().await;
}

#[tokio::test]
async fn the_catalogue_counts_labels_of_connected_runners_that_are_not_revoked() {
    let fleet = Fleet::new().await;
    fleet.runner("a", "1.5.0", "gpu,region=eu").await;
    let cordoned = fleet.runner("b", "1.4.0", "gpu").await;
    let revoked = fleet.runner("c", "1.5.0", "gpu,secret").await;
    let offline = fleet.runner("d", "1.5.0", "region=us").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.age(offline.id, 61.0).await;

    let catalogue: Vec<(String, i64)> = labels::catalogue(fleet.pool())
        .await
        .unwrap()
        .into_iter()
        .map(|available| (available.label, available.runners))
        .collect();
    assert_eq!(
        catalogue,
        [("gpu".to_string(), 2), ("region=eu".to_string(), 1)]
    );
    fleet.remove().await;
}

#[tokio::test]
async fn the_minimum_is_saved_with_who_set_it() {
    let fleet = Fleet::new().await;
    assert_eq!(
        policy::get(fleet.pool()).await.unwrap(),
        policy::Policy::default()
    );
    let actor = fleet.user().await;
    let minimum = Minimum::parse("1.4.0").unwrap();

    let mut transaction = fleet.pool().begin().await.unwrap();
    let saved = policy::save(&mut transaction, Some(&minimum), actor)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(saved.changed());
    assert_eq!(saved.previous, policy::Policy::default());
    assert_eq!(saved.current.minimum, Some(minimum.clone()));
    assert_eq!(saved.current.updated_by, Some(actor));
    let key: Vec<i32> = sqlx::query_scalar("SELECT minimum_version_key FROM fleet_policy WHERE id")
        .fetch_one(fleet.pool())
        .await
        .unwrap();
    assert_eq!(key, [1, 4, 0, 1]);

    let mut transaction = fleet.pool().begin().await.unwrap();
    let unchanged = policy::save(&mut transaction, Some(&minimum), Uuid::nil())
        .await
        .unwrap();
    assert!(!unchanged.changed());
    assert_eq!(unchanged.current, saved.current);
    let cleared = policy::save(&mut transaction, None, actor).await.unwrap();
    transaction.commit().await.unwrap();
    assert!(cleared.changed());
    assert_eq!(cleared.current.minimum, None);
    assert_eq!(policy::get(fleet.pool()).await.unwrap().minimum, None);
    fleet.remove().await;
}

#[tokio::test]
async fn labels_are_saved_once_and_combine_for_a_workspace() {
    let fleet = Fleet::new().await;
    let (organization, workspace) = fleet.workspace().await;
    let actor = fleet.user().await;

    let mut transaction = fleet.pool().begin().await.unwrap();
    let nothing = labels::save_workspace(&mut transaction, workspace, &Labels::default(), actor)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(!nothing.changed());
    assert_eq!(nothing.current, None);
    assert_eq!(
        labels::workspace(fleet.pool(), workspace).await.unwrap(),
        None
    );

    let mut transaction = fleet.pool().begin().await.unwrap();
    let saved = labels::save_organization(&mut transaction, organization, &set("region=eu"), actor)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(saved.changed());
    assert_eq!(saved.previous, None);
    let stored = saved.current.expect("stored labels");
    assert_eq!(stored.labels, set("region=eu"));
    assert_eq!(stored.updated_by, Some(actor));
    assert_eq!(
        labels::organization(fleet.pool(), organization)
            .await
            .unwrap(),
        Some(stored.clone())
    );

    let mut transaction = fleet.pool().begin().await.unwrap();
    let again = labels::save_organization(&mut transaction, organization, &set("region=eu"), actor)
        .await
        .unwrap();
    assert!(!again.changed());
    assert_eq!(again.current, Some(stored.clone()));
    let workspace_saved = labels::save_workspace(&mut transaction, workspace, &set("gpu"), actor)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(workspace_saved.changed());
    assert_eq!(
        labels::required(fleet.pool(), workspace).await.unwrap(),
        set("gpu,region=eu")
    );
    assert!(
        labels::required(fleet.pool(), Uuid::new_v4())
            .await
            .unwrap()
            .is_empty()
    );

    let mut transaction = fleet.pool().begin().await.unwrap();
    let cleared =
        labels::save_organization(&mut transaction, organization, &Labels::default(), actor)
            .await
            .unwrap();
    transaction.commit().await.unwrap();
    assert!(cleared.changed());
    assert_eq!(cleared.previous, Some(stored));
    assert_eq!(
        labels::required(fleet.pool(), workspace).await.unwrap(),
        set("gpu")
    );
    fleet.remove().await;
}

#[tokio::test]
async fn an_organizations_labels_go_with_it() {
    let fleet = Fleet::new().await;
    let (organization, workspace) = fleet.workspace().await;
    fleet
        .require(Some(organization), Some(workspace), "region=eu")
        .await;
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(fleet.pool())
        .await
        .unwrap();
    assert_eq!(
        labels::organization(fleet.pool(), organization)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        labels::workspace(fleet.pool(), workspace).await.unwrap(),
        None
    );
    fleet.remove().await;
}

#[tokio::test]
async fn a_shortfall_names_what_stops_the_runner() {
    let fleet = Fleet::new().await;
    let (organization, workspace) = fleet.workspace().await;
    let (_, unlabelled) = fleet.workspace().await;
    fleet.require(Some(organization), None, "region=eu").await;
    fleet.require(None, Some(workspace), "gpu").await;
    let carrying = fleet.runner("carrying", "1.5.0", "gpu,region=eu").await;
    let lacking = fleet.runner("lacking", "1.5.0", "gpu").await;
    let older = fleet.runner("older", "1.4.0", "gpu,region=eu").await;
    let cordoned = fleet.runner("cordoned", "1.5.0", "gpu,region=eu").await;
    let revoked = fleet.runner("revoked", "1.5.0", "gpu,region=eu").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.minimum(Some("1.5.0")).await;

    let judge = |runner: Option<Uuid>, workspace: Option<Uuid>, mode: Mode| {
        let pool = fleet.pool().clone();
        async move { shortfall(&pool, runner, workspace, mode).await.unwrap() }
    };
    let unattended = Mode::Unattended;
    let interactive = Mode::Interactive;
    assert_eq!(
        judge(Some(carrying.id), Some(workspace), unattended).await,
        None
    );
    assert_eq!(
        judge(Some(lacking.id), Some(workspace), unattended).await,
        Some(Shortfall::Missing {
            labels: set("region=eu")
        })
    );
    assert_eq!(
        judge(Some(lacking.id), Some(workspace), interactive).await,
        None
    );
    assert_eq!(
        judge(Some(lacking.id), Some(unlabelled), unattended).await,
        None
    );
    assert_eq!(judge(Some(lacking.id), None, unattended).await, None);
    assert_eq!(
        judge(Some(older.id), Some(workspace), interactive).await,
        Some(Shortfall::Outdated {
            version: version("1.4.0"),
            minimum: version("1.5.0"),
        })
    );
    assert_eq!(
        judge(Some(cordoned.id), Some(workspace), unattended).await,
        Some(Shortfall::Cordoned)
    );
    assert_eq!(
        judge(Some(cordoned.id), Some(workspace), interactive).await,
        None
    );
    assert_eq!(
        judge(Some(revoked.id), Some(workspace), interactive).await,
        Some(Shortfall::Revoked)
    );
    assert_eq!(
        judge(None, Some(workspace), interactive).await,
        Some(Shortfall::Unregistered)
    );
    assert_eq!(
        judge(Some(Uuid::new_v4()), Some(workspace), interactive).await,
        Some(Shortfall::Unregistered)
    );
    fleet.remove().await;
}
