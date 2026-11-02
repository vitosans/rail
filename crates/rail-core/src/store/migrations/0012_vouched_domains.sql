-- Rail schema v12: the From domains a message's receiving server vouched for (its topmost
-- Authentication-Results: DMARC or an aligned DKIM pass), as a JSON array, so the reader loads
-- a trusted sender's remote pictures only for mail that really came from them. Additive only;
-- NULL, read as none, for every body stored before this version until it is fetched again.
ALTER TABLE messages ADD COLUMN vouched_domains TEXT;
