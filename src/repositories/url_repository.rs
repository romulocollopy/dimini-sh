use crate::domain::entities::url::Url;
use crate::utils::hash::sha256_hex;
use sqlx::Row;
use tracing::{error, instrument};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A stored URL record as returned from the database.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UrlRecord {
    pub id: uuid::Uuid,
    /// JSONB-decoded canonical fields of the Url.
    pub parsed_url: serde_json::Value,
    /// Canonical string form (output of `Url::to_canonical()`).
    pub canonical: String,
    /// SHA-256 hex digest of the canonical string.
    pub url_hash: String,
    /// Short code used for redirect lookups.
    pub short_code: String,
    /// Whether the short code was supplied by the caller (`true`) or generated automatically (`false`).
    pub caller_provided: bool,
}

// ---------------------------------------------------------------------------
// Repository port (trait for dependency injection)
// ---------------------------------------------------------------------------

#[cfg_attr(test, mockall::automock)]
pub trait UrlRepositoryPort {
    fn find_by_short_code(&self, short_code: &str) -> impl std::future::Future<Output = Result<Option<UrlRecord>, RepositoryError>> + Send;

    fn find_by_hash(&self, hash: &str) -> impl std::future::Future<Output = Result<Option<UrlRecord>, RepositoryError>> + Send;

    fn save_with_short_code(&self, url: &Url, short_code: &str, caller_provided: bool) -> impl std::future::Future<Output = Result<uuid::Uuid, RepositoryError>> + Send;
}

/// Repository-level errors.
#[derive(Debug)]
pub enum RepositoryError {
    Database(sqlx::Error),
    Other(String),
}

impl From<sqlx::Error> for RepositoryError {
    fn from(e: sqlx::Error) -> Self {
        RepositoryError::Database(e)
    }
}

/// Postgres-backed repository for `Url` domain entities.
#[derive(Clone)]
pub struct UrlRepository {
    pool: sqlx::PgPool,
}

impl UrlRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        UrlRepository { pool }
    }

    /// Insert a new URL record and return the generated UUID.
    #[instrument(skip(self, url))]
    pub async fn save(&self, url: &Url) -> Result<uuid::Uuid, RepositoryError> {
        let canonical = url.to_canonical();
        let url_hash = sha256_hex(&canonical);
        let parsed_url = serde_json::to_value(url)
            .map_err(|e| RepositoryError::Other(e.to_string()))?;

        let row = sqlx::query(
            r#"
            INSERT INTO urls (canonical, url_hash, parsed_url)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(&canonical)
        .bind(&url_hash)
        .bind(&parsed_url)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| { error!(operation = "save", error = %e, "database error"); RepositoryError::Database(e) })?;

        let id: uuid::Uuid = row.try_get("id")
            .map_err(|e| { error!(operation = "save.try_get_id", error = %e, "database error"); RepositoryError::Database(e) })?;
        Ok(id)
    }

    /// Look up a URL record by its SHA-256 hash.
    #[instrument(skip(self))]
    pub async fn find_by_hash(&self, hash: &str) -> Result<Option<UrlRecord>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, canonical, url_hash, parsed_url, short_code, caller_provided
            FROM urls
            WHERE url_hash = $1
            LIMIT 1
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| { error!(operation = "find_by_hash", error = %e, "database error"); RepositoryError::Database(e) })?;

        Ok(row.map(|r| {
            let parsed_url: serde_json::Value = r.try_get("parsed_url").unwrap_or(serde_json::Value::Null);
            UrlRecord {
                id: r.try_get("id").unwrap(),
                canonical: r.try_get("canonical").unwrap(),
                url_hash: r.try_get("url_hash").unwrap(),
                parsed_url,
                short_code: r.try_get::<Option<String>, _>("short_code").unwrap_or(None).unwrap_or_default(),
                caller_provided: r.try_get("caller_provided").unwrap_or(false),
            }
        }))
    }

    /// Insert a new URL record with a short_code and return the generated UUID.
    #[instrument(skip(self, url))]
    pub async fn save_with_short_code(&self, url: &Url, short_code: &str, caller_provided: bool) -> Result<uuid::Uuid, RepositoryError> {
        let canonical = url.to_canonical();
        let url_hash = sha256_hex(&canonical);
        let parsed_url = serde_json::to_value(url)
            .map_err(|e| RepositoryError::Other(e.to_string()))?;

        let row = sqlx::query(
            r#"
            INSERT INTO urls (canonical, url_hash, parsed_url, short_code, caller_provided)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(&canonical)
        .bind(&url_hash)
        .bind(&parsed_url)
        .bind(short_code)
        .bind(caller_provided)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| { error!(operation = "save_with_short_code", error = %e, "database error"); RepositoryError::Database(e) })?;

        let id: uuid::Uuid = row.try_get("id")
            .map_err(|e| { error!(operation = "save_with_short_code.try_get_id", error = %e, "database error"); RepositoryError::Database(e) })?;
        Ok(id)
    }
}

impl UrlRepositoryPort for UrlRepository {
    async fn save_with_short_code(&self, url: &Url, short_code: &str, caller_provided: bool) -> Result<uuid::Uuid, RepositoryError> {
        UrlRepository::save_with_short_code(self, url, short_code, caller_provided).await
    }

    async fn find_by_hash(&self, hash: &str) -> Result<Option<UrlRecord>, RepositoryError> {
        UrlRepository::find_by_hash(self, hash).await
    }

    #[instrument(skip(self))]
    async fn find_by_short_code(&self, short_code: &str) -> Result<Option<UrlRecord>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, canonical, url_hash, parsed_url, short_code, caller_provided
            FROM urls
            WHERE short_code = $1
            LIMIT 1
            "#,
        )
        .bind(short_code)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| { error!(operation = "find_by_short_code", error = %e, "database error"); RepositoryError::Database(e) })?;

        Ok(row.map(|r| {
            let parsed_url: serde_json::Value = r.try_get("parsed_url").unwrap_or(serde_json::Value::Null);
            UrlRecord {
                id: r.try_get("id").unwrap(),
                canonical: r.try_get("canonical").unwrap(),
                url_hash: r.try_get("url_hash").unwrap(),
                parsed_url,
                short_code: r.try_get::<Option<String>, _>("short_code").unwrap_or(None).unwrap_or_default(),
                caller_provided: r.try_get("caller_provided").unwrap_or(false),
            }
        }))
    }
}



// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::url::Url;
    use sqlx::PgPool;

    /// Helper: connect to the test database using Settings::testing().
    async fn test_pool() -> PgPool {
        let url = crate::settings::Settings::testing().get_database_url().to_string();
        let pool = PgPool::connect(&url)
            .await
            .expect("failed to connect to test database");

        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("failed to run migrations on test database");

        pool
    }

    // -----------------------------------------------------------------------
    // save — happy path
    // -----------------------------------------------------------------------

    /// Saving a valid Url must return a UUID without error.
    // integration test
    #[tokio::test]
    async fn save_valid_url_returns_uuid() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/path?foo=bar").unwrap();

        let result = repo.save(&url).await;

        assert!(result.is_ok(), "expected Ok(uuid), got {:?}", result);
    }

    /// Each call to `save` with the same URL must produce a distinct UUID.
    // integration test
    #[tokio::test]
    async fn save_same_url_twice_returns_different_uuids() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/repeat").unwrap();

        let id_a = repo.save(&url).await.expect("first save failed");
        let id_b = repo.save(&url).await.expect("second save failed");

        assert_ne!(id_a, id_b, "expected distinct UUIDs for two inserts");
    }

    /// A saved record can be retrieved by its SHA-256 hash.
    // integration test
    #[tokio::test]
    async fn find_by_hash_returns_record_after_save() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/findme?a=1").unwrap();

        let _id = repo.save(&url).await.expect("save failed");

        let canonical = url.to_canonical();
        let hash = sha256_hex(&canonical);

        let result = repo.find_by_hash(&hash).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);

        let record = result.unwrap();
        assert!(record.is_some(), "expected Some(record), got None");
    }

    /// The returned UrlRecord must have the correct `canonical` field.
    // integration test
    #[tokio::test]
    async fn saved_record_canonical_matches_url_to_canonical() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/canonical?z=last&a=first").unwrap();
        let expected_canonical = url.to_canonical();

        let _id = repo.save(&url).await.expect("save failed");
        let hash = sha256_hex(&expected_canonical);

        let record = repo
            .find_by_hash(&hash)
            .await
            .expect("find failed")
            .expect("record not found");

        assert_eq!(
            record.canonical, expected_canonical,
            "canonical stored in DB does not match Url::to_canonical()"
        );
    }

    /// The returned UrlRecord must have the correct `url_hash` field.
    // integration test
    #[tokio::test]
    async fn saved_record_url_hash_is_sha256_of_canonical() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/hashcheck").unwrap();
        let canonical = url.to_canonical();
        let expected_hash = sha256_hex(&canonical);

        let _id = repo.save(&url).await.expect("save failed");

        let record = repo
            .find_by_hash(&expected_hash)
            .await
            .expect("find failed")
            .expect("record not found");

        assert_eq!(
            record.url_hash, expected_hash,
            "url_hash in DB does not match SHA-256 of canonical"
        );
    }

    /// The returned UrlRecord must have a `parsed_url` JSONB value containing the scheme.
    // integration test
    #[tokio::test]
    async fn saved_record_parsed_url_contains_scheme() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/jsonb").unwrap();
        let canonical = url.to_canonical();
        let hash = sha256_hex(&canonical);

        let _id = repo.save(&url).await.expect("save failed");

        let record = repo
            .find_by_hash(&hash)
            .await
            .expect("find failed")
            .expect("record not found");

        let scheme = record.parsed_url["scheme"].as_str();
        assert_eq!(
            scheme,
            Some("https"),
            "parsed_url JSONB must contain the scheme field"
        );
    }

    // -----------------------------------------------------------------------
    // find_by_hash — not found
    // -----------------------------------------------------------------------

    /// Searching by a hash that does not exist must return `Ok(None)`.
    // integration test
    #[tokio::test]
    async fn find_by_hash_returns_none_for_unknown_hash() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);

        let result = repo
            .find_by_hash("0000000000000000000000000000000000000000000000000000000000000000")
            .await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert!(
            result.unwrap().is_none(),
            "expected None for unknown hash"
        );
    }

    // -----------------------------------------------------------------------
    // find_by_short_code — integration tests
    // -----------------------------------------------------------------------

    /// Saving a URL with a short_code and then looking it up by that short_code
    /// must return the same record.
    ///
    /// Business rule: `find_by_short_code` is the primary retrieval path for
    /// redirect resolution. A stored short_code must always be retrievable.
    // integration test
    #[tokio::test]
    async fn find_by_short_code_returns_record_after_save_with_short_code() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/short-lookup?v=1").unwrap();
        let short_code = "find01";

        let _id = repo.save_with_short_code(&url, short_code, false).await.expect("save failed");
        let result = repo.find_by_short_code(short_code).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let record = result.unwrap();
        assert!(record.is_some(), "expected Some(record), got None");
    }

    /// The `UrlRecord` returned by `find_by_short_code` must expose the same
    /// short_code that was stored.
    ///
    /// Business rule: the caller must be able to verify which short_code was
    /// resolved to avoid data mixup when caching or logging.
    // integration test
    #[tokio::test]
    async fn find_by_short_code_returned_record_has_correct_short_code() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/short-field?v=2").unwrap();
        let short_code = "find02";

        let _id = repo.save_with_short_code(&url, short_code, false).await.expect("save failed");
        let record = repo
            .find_by_short_code(short_code)
            .await
            .expect("find failed")
            .expect("record not found");

        assert_eq!(
            record.short_code, short_code,
            "short_code on returned record does not match stored value"
        );
    }

    /// Searching for a short_code that was never stored must return `Ok(None)`.
    ///
    /// Business rule: a missing short_code means the link does not exist; the
    /// repository must signal absence as `None`, not as an error.
    // integration test
    #[tokio::test]
    async fn find_by_short_code_returns_none_for_unknown_short_code() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);

        let result = repo.find_by_short_code("never-stored-zzz").await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert!(
            result.unwrap().is_none(),
            "expected None for an unknown short_code"
        );
    }

    /// The `canonical` field on the record returned by `find_by_short_code`
    /// must match `Url::to_canonical()` of the originally saved URL.
    ///
    /// Business rule: callers rely on `canonical` to perform the HTTP redirect;
    /// any mismatch would send users to the wrong destination.
    // integration test
    #[tokio::test]
    async fn find_by_short_code_returned_record_canonical_matches_original_url() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/short-canonical?z=1&a=0").unwrap();
        let short_code = "find03";
        let expected_canonical = url.to_canonical();

        let _id = repo.save_with_short_code(&url, short_code, false).await.expect("save failed");
        let record = repo
            .find_by_short_code(short_code)
            .await
            .expect("find failed")
            .expect("record not found");

        assert_eq!(
            record.canonical, expected_canonical,
            "canonical on returned record does not match Url::to_canonical()"
        );
    }

    // -----------------------------------------------------------------------
    // caller_provided flag — integration tests
    // -----------------------------------------------------------------------

    /// Saving with `caller_provided = true` must persist the flag; retrieving
    /// by short_code must return a record where `caller_provided == true`.
    // integration test
    #[tokio::test]
    async fn saved_record_with_caller_provided_true_has_correct_flag() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/caller-true?v=1").unwrap();
        let short_code = "cptrue1";

        let _id = repo
            .save_with_short_code(&url, short_code, true)
            .await
            .expect("save failed");

        let record = repo
            .find_by_short_code(short_code)
            .await
            .expect("find failed")
            .expect("record not found");

        assert!(
            record.caller_provided,
            "expected caller_provided == true for a caller-supplied short code, got false"
        );
    }

    /// Saving with `caller_provided = false` must persist the flag; retrieving
    /// by short_code must return a record where `caller_provided == false`.
    // integration test
    #[tokio::test]
    async fn saved_record_with_caller_provided_false_has_correct_flag() {
        let pool = test_pool().await;
        let repo = UrlRepository::new(pool);
        let url = Url::parse("https://example.com/caller-false?v=2").unwrap();
        let short_code = "cpfalse1";

        let _id = repo
            .save_with_short_code(&url, short_code, false)
            .await
            .expect("save failed");

        let record = repo
            .find_by_short_code(short_code)
            .await
            .expect("find failed")
            .expect("record not found");

        assert!(
            !record.caller_provided,
            "expected caller_provided == false for a generated short code, got true"
        );
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Compute the SHA-256 hex digest of a string.
    fn sha256_hex(input: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}
