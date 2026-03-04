CREATE TABLE IF NOT EXISTS urls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    canonical TEXT NOT NULL,
    url_hash TEXT NOT NULL,
    parsed_url JSONB NOT NULL,
    short_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
