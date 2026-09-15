//! What deferring the schemas actually gets back, measured rather than claimed.
//!
//! E4's whole case is a number: a chat profile carries forty-odd tools and every
//! schema is sent on every round, which on a 32k window is the single largest
//! fixed cost in the context. This assembles the real catalog for a real
//! workspace and asks `zone_core::context::estimate` — the same function that
//! fills `ContextBreakdown.tools` in production — what each set costs.
//!
//! It asserts a floor rather than an exact figure. An exact one would be a test
//! that fails whenever somebody edits a tool description, which teaches people
//! to update the number rather than to look at it.
mod common;

use common::context::Harness;
use uuid::Uuid;
use zone_core::context::{ContextSource, Policy, estimate};
use zone_server::agent::tools::{ChatTools, WorkspaceScope};

/// The window E4 names. A 32k local model is where this matters; on a large
/// hosted window the same saving is real but nobody notices it.
const WINDOW: u64 = 32_768;
const MODEL: &str = "qwen2.5-coder:7b";

/// At least this much of the tool block has to go, or the mechanism is not
/// worth the round trip it costs a model to load something back.
const LEAST_SAVING: f64 = 0.40;

fn policy() -> Policy {
    Policy {
        limit: Some(WINDOW),
        reserved: 0,
        source: ContextSource::Configured,
    }
}

#[tokio::test]
async fn deferring_the_schemas_is_most_of_the_tool_block_on_a_32k_window() {
    let harness = Harness::new(Some(WINDOW), true, Vec::new()).await;
    let user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner");

    // `preview` rather than `build`: it assembles the same catalog without
    // starting MCP servers, and MCP is exactly the part whose size nobody
    // controls.
    let tools = ChatTools::preview(WorkspaceScope {
        state: common::create_test_state(harness.config.clone(), harness.pool.clone()),
        workspace_id: harness.workspace,
        chat_id: Some(harness.chat),
        user_id: user,
    })
    .await;

    let every = tools.all_definitions().to_vec();
    let exposed = tools.definitions();
    let deferred = tools.deferred().len();

    assert!(
        exposed.len() < every.len(),
        "nothing was deferred, so there is nothing to measure: {} tools",
        every.len()
    );
    assert_eq!(
        exposed.len() + deferred,
        every.len(),
        "every tool is either sent or listed; one that is neither cannot be reached at all"
    );

    let before = estimate(MODEL, &[], Some(&every), &policy(), None)
        .breakdown
        .tools;
    let after = estimate(MODEL, &[], Some(&exposed), &policy(), None)
        .breakdown
        .tools;

    // What the saving is actually made of. A share alone does not say whether
    // deferring bought one heavy schema or twenty light ones, and that is the
    // difference between a mechanism worth keeping and a wash.
    let mut costs: Vec<(&str, u64)> = every
        .iter()
        .filter(|definition| {
            !exposed
                .iter()
                .any(|sent| sent.function.name == definition.function.name)
        })
        .map(|definition| {
            let cost = estimate(
                MODEL,
                &[],
                Some(std::slice::from_ref(definition)),
                &policy(),
                None,
            )
            .breakdown
            .tools;
            (definition.function.name.as_str(), cost)
        })
        .collect();
    costs.sort_by_key(|(_, cost)| std::cmp::Reverse(*cost));

    let saved = before.saturating_sub(after);
    let share = saved as f64 / before as f64;
    println!(
        "ContextBreakdown.tools on a {WINDOW}-token window: {before} before, {after} after — \
         {saved} tokens saved ({:.0}% of the tool block, {:.1}% of the whole window). \
         {} tools sent, {deferred} listed by name.",
        share * 100.0,
        saved as f64 / WINDOW as f64 * 100.0,
        exposed.len(),
    );
    println!(
        "the schemas that cost the most to send every round: {:?}",
        &costs[..costs.len().min(3)]
    );

    assert!(
        share >= LEAST_SAVING,
        "deferring freed only {:.0}% of the tool block ({saved} of {before} tokens). Either the \
         core set has grown to include things a turn rarely needs, or this is no longer worth \
         the round trip a load costs.",
        share * 100.0
    );
}

/// The catalog is what keeps the saving from being a loss: a deferred tool the
/// model cannot name is one it can never ask for.
#[tokio::test]
async fn every_deferred_tool_is_still_reachable_by_name() {
    let harness = Harness::new(Some(WINDOW), true, Vec::new()).await;
    let user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner");

    let tools = ChatTools::preview(WorkspaceScope {
        state: common::create_test_state(harness.config.clone(), harness.pool.clone()),
        workspace_id: harness.workspace,
        chat_id: Some(harness.chat),
        user_id: user,
    })
    .await;

    for listed in tools.deferred() {
        assert!(
            tools.has(&listed.name),
            "{} is listed but the dispatcher does not know it",
            listed.name
        );
        assert!(
            !listed.purpose.trim().is_empty(),
            "{} is listed with no purpose, which is a name the model cannot choose between",
            listed.name
        );
        assert!(
            !listed.purpose.contains('\n'),
            "{} spans lines; the catalog is carried every round and has to stay one line each",
            listed.name
        );
    }

    // And the two that fetch the rest are never themselves deferred, which
    // would strand everything behind them.
    let held: Vec<&str> = tools
        .deferred()
        .iter()
        .map(|listed| listed.name.as_str())
        .collect();
    assert!(
        !held.contains(&"load_tools"),
        "load_tools cannot be deferred"
    );
    assert!(
        !held.contains(&"search_tools"),
        "search_tools cannot be deferred"
    );
}
