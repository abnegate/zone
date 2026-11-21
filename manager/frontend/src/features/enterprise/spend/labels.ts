export const PAGE_TITLE = 'Spend';

export const PAGE_DESCRIPTION =
  "What this organization spends on AI through Zone's LiteLLM, by day, workspace, person, model and provider.";

export const LOADING_LABEL = 'Loading spend...';

export const RETRY_LABEL = 'Retry';

export const UPGRADE_NOTICE = 'Spend reports and budgets are available on Team.';

export const UPGRADE_DETAIL =
  'Add a Team license to see what each workspace and person spends on AI, and to set monthly budgets with optional hard limits.';

export const UPGRADE_ACTION = 'Go to License';

export const LAPSED_NOTICE =
  "Your Team license has lapsed. Reports stay readable and hard limits stay enforced, but budgets can't be changed until a new license is added.";

export const LAPSED_ACTION = 'Renew on the License page';

export const HARD_LIMIT_NOTE =
  "Hard limits stop requests through Zone's LiteLLM only. Coding agents and direct providers keep running.";

export const UNCOUNTED_FOOTNOTE =
  "Not counted: coding-agent subscriptions, direct provider keys, other endpoints, workspace keys saved for this server's LiteLLM, chats that pick their own provider, requests outside a workspace, and embeddings. Local models show tokens at $0 unless LiteLLM prices them.";

export const AI_SETTINGS_LINK = 'See coding agents and providers in Settings › AI';

export const FROM_LABEL = 'From';

export const TO_LABEL = 'To';

export const WORKSPACE_FILTER_LABEL = 'Workspace';

export const PERSON_FILTER_LABEL = 'Person';

export const ALL_WORKSPACES = 'All workspaces';

export const EVERYONE = 'Everyone';

export const DOWNLOAD_LABEL = 'Download CSV';

export const DOWNLOADED_LABEL = 'Saved';

export const DOWNLOAD_INTERRUPTED = 'The download was interrupted. Try again.';

export const RANGE_LEGEND = 'Report range and filters';

export const SPEND_TILE = 'Spend';

export const REQUESTS_TILE = 'Requests';

export const PROMPT_TOKENS_TILE = 'Prompt tokens';

export const COMPLETION_TOKENS_TILE = 'Completion tokens';

export const COUNTED_THROUGH = 'Counted through';

export const NOTHING_COUNTED =
  'Nothing has been counted yet. Spend shows up about a minute after a request finishes.';

export const CHART_TITLE = 'Daily spend';

export const CHART_CAPTION = 'Spend per day';

export const CHART_NOTE = 'Days in UTC';

export const NO_SPEND = 'No priced spend in this range.';

export const DAY_COLUMN = 'Day';

export const NAME_COLUMN = 'Name';

export const SPEND_COLUMN = 'Spend';

export const REQUESTS_COLUMN = 'Requests';

export const TOKENS_COLUMN = 'Tokens';

export const WORKSPACES_TITLE = 'By workspace';

export const PEOPLE_TITLE = 'By person';

export const MODELS_TITLE = 'By model';

export const PROVIDERS_TITLE = 'By provider';

export const AUTOMATION_LABEL = 'Automation';

export const AUTOMATION_DETAIL = 'Pull requests, tasks and other work nobody sent from a chat';

export const DELETED_WORKSPACE = 'Deleted workspace';

export const UNKNOWN_PERSON = 'Former member';

export const UNKNOWN_PROVIDER = 'Unknown provider';

export const NOTHING_IN_RANGE = 'Nothing in this range.';

export const UNCOUNTED_TITLE = 'Not counted';

export const UNCOUNTED_DESCRIPTION =
  "Traffic that bypasses Zone's LiteLLM or can't be tied to a workspace has no number here.";

export const UNCOUNTED_PRESENT = 'In this organization right now:';

export const BUDGETS_TITLE = 'Monthly budgets';

export const NO_BUDGETS = 'No budgets yet.';

export const READ_ONLY = 'Only organization owners and admins can change budgets.';

export const ADD_TITLE = 'Add a budget';

export const EDIT_TITLE = 'Edit budget';

export const SCOPE_LABEL = 'Applies to';

export const WORKSPACE_PICKER_LABEL = 'Workspace';

export const PERSON_PICKER_LABEL = 'Person';

export const CHOOSE_WORKSPACE = 'Choose a workspace';

export const CHOOSE_PERSON = 'Choose a person';

export const LIMIT_LABEL = 'Monthly limit (USD)';

export const LIMIT_PLACEHOLDER = '50.00';

export const THRESHOLD_LABEL = 'Warn at (% of the limit)';

export const THRESHOLD_HELP = 'Owners and admins get a notice once spend reaches this share.';

export const HARD_LABEL = 'Stop requests once the budget is used up';

export const ADD_LABEL = 'Add budget';

export const SAVE_LABEL = 'Save budget';

export const CANCEL_LABEL = 'Cancel';

export const EDIT_LABEL = 'Edit';

export const DELETE_LABEL = 'Delete';

export const DELETE_TITLE = 'Delete budget';

export const DELETE_CONFIRM_LABEL = 'Delete budget';

export const HARD_BADGE = 'Hard limit';

export const SOFT_BADGE = 'Soft limit';

export const ORGANIZATION_TARGET = 'Whole organization';

export const UNKNOWN_WORKSPACE = 'Unknown workspace';
