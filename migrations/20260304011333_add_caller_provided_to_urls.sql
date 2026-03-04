-- Add caller_provided column to urls table if it doesn't exist
ALTER TABLE urls ADD COLUMN IF NOT EXISTS caller_provided BOOLEAN NOT NULL DEFAULT FALSE;
