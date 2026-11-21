//! Tells an organization's admins that a budget reached its alert threshold or its limit, once a
//! month: by mail, one admin at a time, and through the server's notification channels.

use abnegate_notify::{Fanout, Notification, Severity};
use serde_json::json;
use zone_enterprise::spend::{Month, Scope};

use super::error::Error;
use crate::db::audit::{self, AuditContext, actions, resources};
use crate::db::organizations;
use crate::db::spend::budgets::{self, Announcement, BudgetRow, Crossing, Recipient};
use crate::services::mail::{SEND_TIMEOUT, Template};
use crate::state::AppState;

/// Where the console keeps an organization's spend and budgets.
pub const SPEND_PATH: &str = "/settings/organization/spend";

/// Sends `crossing`'s notice for `month` and records it as sent, once it reached someone or
/// there was no one to send it to. Returns whether it was recorded; a notice every configured
/// channel failed to take is sent again on the next sweep.
pub async fn announce(
    state: &AppState,
    fanout: &Fanout,
    crossing: &Crossing,
    month: Month,
) -> Result<bool, Error> {
    let row = &crossing.row;
    let pool = state.db();
    let organization = organizations::get_organization(pool, row.organization_id)
        .await?
        .map(|organization| organization.name)
        .unwrap_or_default();
    let notice = Notice {
        announcement: crossing.announcement,
        budget: label(row, &organization),
        spent: format!("${}", row.spent.dollars()),
        limit: format!("${}", row.budget.limit.dollars()),
        link: format!(
            "{}{SPEND_PATH}",
            state.config().app_base_url.trim_end_matches('/')
        ),
        hard: row.budget.hard,
    };
    let recipients = budgets::recipients(pool, row.organization_id, row.target.workspace).await?;
    let mailed = mail(state, &notice, &recipients).await;
    let broadcast = broadcast(fanout, &notice).await;
    if !delivered(mailed, broadcast) {
        tracing::warn!(
            budget = %row.id,
            "No channel took a budget notice; it is sent again on the next sweep"
        );
        return Ok(false);
    }
    budgets::announce(pool, row.id, crossing.announcement, month).await?;
    record(state, crossing, month).await;
    Ok(true)
}

/// Whether a notice counts as sent, given whether mail and the notification channels took it,
/// none for a channel that isn't configured: when any took it, or when none is configured.
pub(super) fn delivered(mailed: Option<bool>, broadcast: Option<bool>) -> bool {
    match (mailed, broadcast) {
        (None, None) => true,
        (mailed, broadcast) => mailed == Some(true) || broadcast == Some(true),
    }
}

/// What a notice says, before it is addressed to anyone.
pub(super) struct Notice {
    pub(super) announcement: Announcement,
    pub(super) budget: String,
    pub(super) spent: String,
    pub(super) limit: String,
    pub(super) link: String,
    pub(super) hard: bool,
}

impl Notice {
    pub(super) fn template<'a>(&'a self, name: &'a str) -> Template<'a> {
        match self.announcement {
            Announcement::Warning => Template::BudgetWarning {
                name,
                budget: &self.budget,
                spent: &self.spent,
                limit: &self.limit,
                link: &self.link,
            },
            Announcement::Exhaustion => Template::BudgetExhausted {
                name,
                budget: &self.budget,
                spent: &self.spent,
                limit: &self.limit,
                link: &self.link,
                hard: self.hard,
            },
        }
    }

    pub(super) fn notification(&self) -> Notification {
        let (title, severity) = match self.announcement {
            Announcement::Warning => ("AI budget alert", Severity::Warning),
            Announcement::Exhaustion => ("AI budget used up", Severity::Error),
        };
        Notification::new(
            format!("{title}: {}", self.budget),
            format!(
                "{} of the {} monthly limit is spent this month.",
                self.spent, self.limit
            ),
        )
        .severity(severity)
        .link(self.link.as_str())
        .field("Hard limit", if self.hard { "Yes" } else { "No" })
    }
}

/// Which budget a notice is about, in words: its organization, its workspace or its member.
pub(super) fn label(row: &BudgetRow, organization: &str) -> String {
    match row.target.scope {
        Scope::Organization => format!("the {organization} organization"),
        Scope::Workspace => format!(
            "the {} workspace in {organization}",
            row.workspace_name.as_deref().unwrap_or_default()
        ),
        Scope::User => {
            let email = row.user_email.as_deref().unwrap_or_default();
            match row.user_name.as_deref() {
                Some(name) => format!("{name} ({email}) in {organization}"),
                None => format!("{email} in {organization}"),
            }
        }
    }
}

/// Mails each recipient in turn, waiting on each send. None when there is no mail or no one to
/// mail, otherwise whether anyone was reached.
async fn mail(state: &AppState, notice: &Notice, recipients: &[Recipient]) -> Option<bool> {
    let mail = state.mail()?;
    if recipients.is_empty() {
        return None;
    }
    let mut reached = false;
    for recipient in recipients {
        let template = notice.template(&recipient.name);
        match tokio::time::timeout(SEND_TIMEOUT, mail.send(&recipient.email, &template)).await {
            Ok(Ok(())) => reached = true,
            Ok(Err(error)) => tracing::warn!(%error, "A budget notice did not go out by mail"),
            Err(_) => tracing::warn!(
                "A budget notice did not go out by mail: the relay did not answer in time"
            ),
        }
    }
    Some(reached)
}

/// None when no notification channel is configured, otherwise whether any took the notice.
async fn broadcast(fanout: &Fanout, notice: &Notice) -> Option<bool> {
    if fanout.is_empty() {
        return None;
    }
    let report = fanout.deliver(&notice.notification()).await;
    for failure in report.failures() {
        tracing::warn!(
            channel = failure.name(),
            error = ?failure.error(),
            "A budget notice did not go out"
        );
    }
    Some(report.any_delivered())
}

async fn record(state: &AppState, crossing: &Crossing, month: Month) {
    let row = &crossing.row;
    let action = match crossing.announcement {
        Announcement::Warning => actions::BUDGET_THRESHOLD_REACHED,
        Announcement::Exhaustion => actions::BUDGET_EXHAUSTED,
    };
    let context = AuditContext {
        org_id: Some(row.organization_id),
        workspace_id: row.target.workspace,
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    let values = json!({
        "month": month.label(),
        "spent_microdollars": row.spent.0,
        "limit_microdollars": row.budget.limit.0,
        "hard": row.budget.hard,
    });
    if let Err(error) = audit::log_action(
        state.db(),
        &context,
        action,
        resources::BUDGET,
        Some(row.id),
        None,
        Some(values),
    )
    .await
    {
        tracing::error!(%error, budget = %row.id, action, "Failed to audit a budget notice");
    }
}
