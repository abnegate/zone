-- A workspace created through POST /organizations/{id}/workspaces enrolled its
-- creator as admin rather than owner, and "only owners can remove admins or
-- owners" then left nobody able to remove or demote an admin in it. New
-- workspaces now enrol their creator as owner; these are the ones already on
-- disk.
--
-- One member per ownerless workspace, chosen deterministically: the creator
-- first -- self-enrolment leaves invited_by NULL -- then the highest role, then
-- the earliest joiner, then the smallest id, so the outcome does not depend on
-- scan order.
UPDATE workspace_members AS promoted
SET role = 'owner', updated_at = NOW()
WHERE promoted.id IN (
    SELECT DISTINCT ON (candidate.workspace_id) candidate.id
    FROM workspace_members AS candidate
    JOIN workspaces AS workspace ON workspace.id = candidate.workspace_id
    WHERE candidate.is_active
      AND workspace.is_active
      AND NOT EXISTS (
          SELECT 1
          FROM workspace_members AS existing
          WHERE existing.workspace_id = candidate.workspace_id
            AND existing.is_active
            AND existing.role = 'owner'
      )
    ORDER BY
        candidate.workspace_id,
        (candidate.invited_by IS NULL) DESC,
        CASE candidate.role
            WHEN 'admin' THEN 0
            WHEN 'member' THEN 1
            WHEN 'viewer' THEN 2
            ELSE 3
        END,
        candidate.created_at,
        candidate.id
);
