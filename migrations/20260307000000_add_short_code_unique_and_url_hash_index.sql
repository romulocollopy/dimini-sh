ALTER TABLE urls ADD CONSTRAINT urls_short_code_unique UNIQUE (short_code);
CREATE INDEX urls_url_hash_idx ON urls (url_hash);
