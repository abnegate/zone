-- Where a firing lives between being claimed and being answered.
--
-- A reminder carrying fixed content is delivered by the transaction that claims
-- it: the message is inserted and the schedule moves on, together or not at
-- all. A reminder carrying a prompt cannot be, because a prompt is a turn and a
-- turn is a model call, which is not something to hold a row lock across. So
-- the claim commits first and the turn runs after it, and the gap between those
-- two points is where a firing can be lost -- a process that exits there leaves
-- a reminder marked fired and a chat with nothing in it, and the occurrence is
-- gone, because the schedule has already moved past it.
--
-- A row here spans that gap. It is written inside the claim's transaction, so
-- it is exactly as durable as the schedule moving on, and it is deleted once
-- the turn has run. A process that dies in between leaves the row behind, and
-- the next sweep finds it because the claim on it has gone stale.
--
-- The contract is therefore at-least-once: a turn that ran and was never marked
-- done runs again. That is the right side to err on for a firing somebody asked
-- for -- a repeated question is a nuisance and a missing answer is a broken
-- promise -- and `attempts` is what keeps a firing that fails the same way
-- every time from being retried for ever.
CREATE TABLE reminder_turns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    reminder_id UUID NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    prompt TEXT NOT NULL CHECK (length(trim(prompt)) > 0),
    attempts INTEGER NOT NULL DEFAULT 0,
    claimed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The sweep takes the oldest claimable row and takes one at a time; cancelling
-- a reminder drops whatever it still owes by reminder.
CREATE INDEX reminder_turns_queue ON reminder_turns (created_at, id);
CREATE INDEX reminder_turns_reminder ON reminder_turns (reminder_id);
