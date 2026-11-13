-- Rail schema v14: remember that an SMTP relay accepted an outbox row's message, so a retry
-- after a Sent-copy failure (or a daemon restart) never submits it again.
ALTER TABLE outbox ADD COLUMN relay_accepted_at INTEGER;
