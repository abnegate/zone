-- Claim the next task from the queue for a worker
SELECT * FROM claim_next_task($1)
