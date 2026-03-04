use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "URL parse error: {}", self.0)
    }
}

// ---------------------------------------------------------------------------
// QueryString
// ---------------------------------------------------------------------------

/// A parsed, order-independent query string.
///
/// Internally stores key-value pairs. Equality is order-independent.
/// Serialises to a canonical string with keys sorted alphabetically.
#[derive(Debug, Clone, Serialize)]
pub struct QueryString {
    pairs: HashMap<String, String>,
}

impl QueryString {
    /// Parse a raw query string (without the leading `?`).
    /// Returns an empty `QueryString` for an empty input.
    pub fn parse(raw: &str) -> Self {
        let mut pairs = HashMap::new();
        if !raw.is_empty() {
            for part in raw.split('&') {
                if let Some((k, v)) = part.split_once('=') {
                    pairs.insert(k.to_string(), v.to_string());
                } else if !part.is_empty() {
                    pairs.insert(part.to_string(), String::new());
                }
            }
        }
        QueryString { pairs }
    }

    /// Serialise to canonical form: keys sorted alphabetically, joined with `&`.
    pub fn to_canonical(&self) -> String {
        let mut keys: Vec<&String> = self.pairs.keys().collect();
        keys.sort();
        keys.iter()
            .map(|k| {
                let v = &self.pairs[*k];
                if v.is_empty() {
                    k.to_string()
                } else {
                    format!("{}={}", k, v)
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }
}

impl PartialEq for QueryString {
    fn eq(&self, other: &Self) -> bool {
        self.pairs == other.pairs
    }
}

impl Eq for QueryString {}

impl Hash for QueryString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_canonical().hash(state);
    }
}

impl fmt::Display for QueryString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_canonical())
    }
}

// ---------------------------------------------------------------------------
// Url
// ---------------------------------------------------------------------------

/// A fully parsed and normalised URL.
///
/// All fields are normalised at construction time:
/// - `scheme` and `host` are lowercased.
/// - `port` is `None` if not explicitly present in the input string.
/// - `path` has its trailing slash stripped unless it is the root `/`.
/// - `query` is a `QueryString` (order-independent equality, canonical serialisation).
/// - `fragment` is `None` when absent.
///
/// Equality, hashing, and `Display` are all based on the canonical string form.
#[derive(Debug, Clone, Serialize)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: QueryString,
    pub fragment: Option<String>,
}

/// Extract the port explicitly written in the URL string, if any.
///
/// The `url` crate's `port()` returns `None` for known-default ports (e.g. 80 for http)
/// even when they are explicitly written. We parse the raw string to preserve them.
fn extract_explicit_port(input: &str) -> Option<u16> {
    // Find end of scheme://
    let after_scheme = input.find("://")?;
    let authority_start = after_scheme + 3;
    let rest = &input[authority_start..];
    // Authority ends at /, ?, # or end of string
    let auth_len = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
    let authority = &rest[..auth_len];
    // Strip userinfo (user:pass@)
    let authority = if let Some(at) = authority.rfind('@') {
        &authority[at + 1..]
    } else {
        authority
    };
    // Handle IPv6 address like [::1]:8080
    if authority.starts_with('[') {
        let bracket_end = authority.find(']')?;
        let after_bracket = &authority[bracket_end + 1..];
        return if after_bracket.starts_with(':') {
            after_bracket[1..].parse::<u16>().ok()
        } else {
            None
        };
    }
    // For hostname/IPv4: port is after the last colon
    authority.rfind(':').and_then(|colon| authority[colon + 1..].parse::<u16>().ok())
}

impl Url {
    /// Parse a URL strictly — requires an explicit scheme (e.g. `https://`).
    ///
    /// Unlike `parse`, this does not default to `http://` for scheme-less input.
    /// Use this when you need to reject bare hostnames or any input that omits the scheme.
    pub fn parse_strict(input: &str) -> Result<Self, ParseError> {
        if !input.contains("://") {
            return Err(ParseError("missing scheme".to_string()));
        }
        Self::parse(input)
    }

    /// Parse a URL from a string slice.
    ///
    /// Returns `Err(ParseError)` for any input that is not a valid URL.
    /// The `url` crate is used internally but its types do not leak into public interfaces.
    /// Scheme-less input (e.g. `example.com/path`) defaults to `http://`.
    /// Use `parse_strict` if you need to require an explicit scheme.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        // Requirement 2: no scheme defaults to http
        let owned;
        let input = if !input.contains("://") {
            owned = format!("http://{}", input);
            owned.as_str()
        } else {
            input
        };
        let parsed = url::Url::parse(input)
            .map_err(|e| ParseError(e.to_string()))?;

        let scheme = parsed.scheme().to_lowercase();

        // Reject empty authority (e.g. "https:///path") — the url crate misparses
        // "https:///path" as host="path", so we validate the raw input ourselves.
        if let Some(after_slashes) = input.find("://").map(|i| &input[i + 3..]) {
            let auth_end = after_slashes.find(|c| c == '/' || c == '?' || c == '#')
                .unwrap_or(after_slashes.len());
            // Strip userinfo
            let authority = &after_slashes[..auth_end];
            let host_part = if let Some(at) = authority.rfind('@') {
                &authority[at + 1..]
            } else {
                authority
            };
            // Strip port from host_part for the emptiness check
            let hostname = if host_part.starts_with('[') {
                host_part
            } else if let Some(colon) = host_part.rfind(':') {
                &host_part[..colon]
            } else {
                host_part
            };
            if hostname.is_empty() {
                return Err(ParseError("missing host".to_string()));
            }
        }

        let host = parsed
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| ParseError("missing host".to_string()))?
            .to_lowercase();

        // Use raw string parsing to preserve explicitly-written default ports
        // (e.g. :80 on http). The url crate's port() strips known defaults.
        let port = extract_explicit_port(input);

        let raw_path = parsed.path();
        let path = if raw_path.len() > 1 && raw_path.ends_with('/') {
            raw_path.trim_end_matches('/').to_string()
        } else {
            raw_path.to_string()
        };

        let query = QueryString::parse(parsed.query().unwrap_or(""));

        let fragment = parsed.fragment().map(|f| f.to_string());

        Ok(Url { scheme, host, port, path, query, fragment })
    }

    /// Produce the canonical string form:
    /// `scheme://host[:port]path[?sorted_query][#fragment]`
    pub fn to_canonical(&self) -> String {
        let mut s = format!("{}://{}", self.scheme, self.host);
        if let Some(port) = self.port {
            let is_default = (self.scheme == "http" && port == 80)
                || (self.scheme == "https" && port == 443);
            if !is_default {
                s.push_str(&format!(":{}", port));
            }
        }
        s.push_str(&self.path);
        let q = self.query.to_canonical();
        if !q.is_empty() {
            s.push('?');
            s.push_str(&q);
        }
        if let Some(ref frag) = self.fragment {
            s.push('#');
            s.push_str(frag);
        }
        s
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_canonical())
    }
}

impl PartialEq for Url {
    fn eq(&self, other: &Self) -> bool {
        self.to_canonical() == other.to_canonical()
    }
}

impl Eq for Url {}

impl Hash for Url {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_canonical().hash(state);
    }
}

impl PartialEq<&str> for Url {
    fn eq(&self, other: &&str) -> bool {
        match Url::parse(other) {
            Ok(other_url) => self.to_canonical() == other_url.to_canonical(),
            Err(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of<T: Hash>(val: &T) -> u64 {
        let mut h = DefaultHasher::new();
        val.hash(&mut h);
        h.finish()
    }

    // -----------------------------------------------------------------------
    // Parsing — valid input
    // -----------------------------------------------------------------------

    /// A well-formed HTTPS URL must parse without error.
    ///
    /// Business rule: `Url::parse` is the sole constructor; valid input must succeed.
    #[test]
    fn parse_valid_https_url_succeeds() {
        let result = Url::parse("https://example.com/path");
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
    }

    /// A well-formed HTTP URL must also parse successfully.
    #[test]
    fn parse_valid_http_url_succeeds() {
        let result = Url::parse("http://example.com/");
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
    }

    // -----------------------------------------------------------------------
    // Parsing — invalid input
    // -----------------------------------------------------------------------

    /// A completely empty string is not a valid URL.
    ///
    /// Business rule: `Url::parse` must return `Err` for any input that does
    /// not conform to the URL grammar.
    #[test]
    fn parse_empty_string_returns_error() {
        assert!(Url::parse("").is_err());
    }

    /// No scheme defaults to http (Requirement 2).
    ///
    /// Business rule: `Url::parse("example.com/path")` must succeed and be
    /// treated as `http://example.com/path`.
    #[test]
    fn parse_url_without_scheme_defaults_to_http() {
        let result = Url::parse("example.com/path");
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let url = result.unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/path");
    }

    /// A string with no host is not a valid URL.
    #[test]
    fn parse_url_without_host_returns_error() {
        assert!(Url::parse("https:///path").is_err());
    }

    // -----------------------------------------------------------------------
    // Normalised fields — scheme
    // -----------------------------------------------------------------------

    /// The `scheme` field must be stored in lowercase.
    ///
    /// Business rule: scheme normalisation ensures that `HTTP` and `http`
    /// are treated as the same scheme throughout the system.
    #[test]
    fn scheme_is_lowercased() {
        let url = Url::parse("HTTPS://example.com/").unwrap();
        assert_eq!(url.scheme, "https");
    }

    /// The `scheme` field for a standard HTTP URL must be `"http"`.
    #[test]
    fn scheme_for_http_url_is_http() {
        let url = Url::parse("http://example.com/").unwrap();
        assert_eq!(url.scheme, "http");
    }

    // -----------------------------------------------------------------------
    // Normalised fields — host
    // -----------------------------------------------------------------------

    /// The `host` field must be stored in lowercase.
    ///
    /// Business rule: host normalisation ensures that `Example.COM` and
    /// `example.com` refer to the same server.
    #[test]
    fn host_is_lowercased() {
        let url = Url::parse("https://Example.COM/").unwrap();
        assert_eq!(url.host, "example.com");
    }

    /// The `host` field must preserve subdomains.
    #[test]
    fn host_preserves_subdomain() {
        let url = Url::parse("https://api.example.com/").unwrap();
        assert_eq!(url.host, "api.example.com");
    }

    // -----------------------------------------------------------------------
    // Normalised fields — port
    // -----------------------------------------------------------------------

    /// When no port is present in the URL string, `port` must be `None`.
    ///
    /// Business rule: `port` is `None` exactly when the URL string did not
    /// contain an explicit port. Default ports must NOT be inferred.
    #[test]
    fn port_is_none_when_not_in_url() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url.port, None);
    }

    /// When an explicit port is present, `port` must be `Some(n)`.
    #[test]
    fn port_is_some_when_explicit() {
        let url = Url::parse("https://example.com:8443/").unwrap();
        assert_eq!(url.port, Some(8443));
    }

    /// Port 80 is stored as-is when explicit (no default-port stripping).
    #[test]
    fn explicit_port_80_is_preserved() {
        let url = Url::parse("http://example.com:80/").unwrap();
        assert_eq!(url.port, Some(80));
    }

    // -----------------------------------------------------------------------
    // Normalised fields — path
    // -----------------------------------------------------------------------

    /// A trailing slash on a non-root path must be stripped.
    ///
    /// Business rule: `/foo/bar/` and `/foo/bar` refer to the same resource;
    /// the canonical form has no trailing slash.
    #[test]
    fn trailing_slash_on_non_root_path_is_stripped() {
        let url = Url::parse("https://example.com/foo/bar/").unwrap();
        assert_eq!(url.path, "/foo/bar");
    }

    /// The root path `/` must be preserved as-is.
    ///
    /// Business rule: stripping the trailing slash from `/` would produce an
    /// empty path, which is not a valid path component. The root is kept.
    #[test]
    fn root_path_is_preserved() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url.path, "/");
    }

    /// A path without a trailing slash must not be modified.
    #[test]
    fn path_without_trailing_slash_is_unchanged() {
        let url = Url::parse("https://example.com/foo/bar").unwrap();
        assert_eq!(url.path, "/foo/bar");
    }

    // -----------------------------------------------------------------------
    // Normalised fields — query
    // -----------------------------------------------------------------------

    /// A URL with no query string must produce an empty `QueryString`.
    #[test]
    fn query_is_empty_when_no_query_string() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url.query, QueryString::parse(""));
    }

    /// A URL with a query string must parse key-value pairs into `QueryString`.
    #[test]
    fn query_parses_key_value_pairs() {
        let url = Url::parse("https://example.com/?foo=bar").unwrap();
        assert_eq!(url.query, QueryString::parse("foo=bar"));
    }

    // -----------------------------------------------------------------------
    // Normalised fields — fragment
    // -----------------------------------------------------------------------

    /// A URL with no fragment must have `fragment` equal to `None`.
    #[test]
    fn fragment_is_none_when_absent() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url.fragment, None);
    }

    /// A URL with a fragment must expose it as `Some(String)`.
    #[test]
    fn fragment_is_some_when_present() {
        let url = Url::parse("https://example.com/#section-1").unwrap();
        assert_eq!(url.fragment, Some("section-1".to_string()));
    }

    // -----------------------------------------------------------------------
    // QueryString — order-independent equality
    // -----------------------------------------------------------------------

    /// Two query strings with the same pairs in different order must be equal.
    ///
    /// Business rule: `?foo=bar&spam=eggs` and `?spam=eggs&foo=bar` are
    /// semantically identical. Order must not affect equality.
    #[test]
    fn query_string_equality_is_order_independent() {
        let a = QueryString::parse("foo=bar&spam=eggs");
        let b = QueryString::parse("spam=eggs&foo=bar");
        assert_eq!(a, b);
    }

    /// A query string is equal to itself.
    #[test]
    fn query_string_reflexive_equality() {
        let q = QueryString::parse("foo=bar");
        assert_eq!(q, q.clone());
    }

    /// Query strings with different values are not equal.
    #[test]
    fn query_strings_with_different_values_are_not_equal() {
        let a = QueryString::parse("foo=bar");
        let b = QueryString::parse("foo=baz");
        assert_ne!(a, b);
    }

    /// Query strings with different keys are not equal.
    #[test]
    fn query_strings_with_different_keys_are_not_equal() {
        let a = QueryString::parse("foo=bar");
        let b = QueryString::parse("qux=bar");
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // QueryString — canonical serialisation
    // -----------------------------------------------------------------------

    /// `to_canonical` must sort keys alphabetically.
    ///
    /// Business rule: canonical form enables consistent comparison and caching.
    /// Key order in the original string must not affect the canonical output.
    #[test]
    fn query_string_canonical_sorts_keys_alphabetically() {
        let q = QueryString::parse("spam=eggs&foo=bar");
        assert_eq!(q.to_canonical(), "foo=bar&spam=eggs");
    }

    /// A single key-value pair serialises correctly.
    #[test]
    fn query_string_canonical_single_pair() {
        let q = QueryString::parse("foo=bar");
        assert_eq!(q.to_canonical(), "foo=bar");
    }

    /// An empty query string serialises to an empty string.
    #[test]
    fn query_string_canonical_empty_is_empty_string() {
        let q = QueryString::parse("");
        assert_eq!(q.to_canonical(), "");
    }

    // -----------------------------------------------------------------------
    // Url equality — canonical-form based
    // -----------------------------------------------------------------------

    /// Two URLs that differ only in query parameter order must be equal.
    ///
    /// Business rule: URL equality is based on canonical form. Canonical form
    /// uses alphabetically sorted query keys, making order irrelevant.
    #[test]
    fn urls_equal_when_query_order_differs() {
        let a = Url::parse("http://example.com/?foo=bar&spam=eggs").unwrap();
        let b = Url::parse("http://example.com/?spam=eggs&foo=bar").unwrap();
        assert_eq!(a, b);
    }

    /// Two identical URLs must be equal.
    #[test]
    fn identical_urls_are_equal() {
        let a = Url::parse("https://example.com/path?key=val#frag").unwrap();
        let b = Url::parse("https://example.com/path?key=val#frag").unwrap();
        assert_eq!(a, b);
    }

    /// URLs with different hosts must not be equal.
    #[test]
    fn urls_with_different_hosts_are_not_equal() {
        let a = Url::parse("https://example.com/").unwrap();
        let b = Url::parse("https://other.com/").unwrap();
        assert_ne!(a, b);
    }

    /// URLs with different paths must not be equal.
    #[test]
    fn urls_with_different_paths_are_not_equal() {
        let a = Url::parse("https://example.com/foo").unwrap();
        let b = Url::parse("https://example.com/bar").unwrap();
        assert_ne!(a, b);
    }

    /// URLs with different schemes must not be equal.
    #[test]
    fn urls_with_different_schemes_are_not_equal() {
        let a = Url::parse("http://example.com/").unwrap();
        let b = Url::parse("https://example.com/").unwrap();
        assert_ne!(a, b);
    }

    /// URLs with different ports must not be equal.
    #[test]
    fn urls_with_different_ports_are_not_equal() {
        let a = Url::parse("https://example.com:8080/").unwrap();
        let b = Url::parse("https://example.com:9090/").unwrap();
        assert_ne!(a, b);
    }

    /// With RFC port normalisation, a URL with an explicit default port is equal to one without.
    #[test]
    fn url_with_explicit_default_port_equals_url_without_port() {
        let a = Url::parse("https://example.com/").unwrap();
        let b = Url::parse("https://example.com:443/").unwrap();
        assert_eq!(a, b);
    }

    /// The `port` field preserves the explicitly written port even when it is a
    /// scheme default and therefore omitted from the canonical form.
    #[test]
    fn explicit_default_port_is_stored_but_not_in_canonical() {
        let url = Url::parse("http://example.com:80/").unwrap();
        assert_eq!(url.port, Some(80));
        assert_eq!(url.to_string(), "http://example.com/");
    }

    // -----------------------------------------------------------------------
    // Display / to_string — canonical form
    // -----------------------------------------------------------------------

    /// `to_string()` must produce the canonical URL form.
    ///
    /// Business rule: canonical form is `scheme://host[:port]path[?sorted_query][#fragment]`.
    /// This form is used for equality, hashing, caching, and storage.
    #[test]
    fn display_produces_canonical_form() {
        let url = Url::parse("http://example.com/?foo=bar&spam=eggs").unwrap();
        assert_eq!(url.to_string(), "http://example.com/?foo=bar&spam=eggs");
    }

    /// A URL with an explicit port is rendered with the port in `to_string()`.
    #[test]
    fn display_includes_explicit_port() {
        let url = Url::parse("https://example.com:8443/path").unwrap();
        assert_eq!(url.to_string(), "https://example.com:8443/path");
    }

    /// A URL with a fragment is rendered with the fragment in `to_string()`.
    #[test]
    fn display_includes_fragment() {
        let url = Url::parse("https://example.com/#anchor").unwrap();
        assert_eq!(url.to_string(), "https://example.com/#anchor");
    }

    /// Canonical form sorts query keys alphabetically in `to_string()`.
    #[test]
    fn display_sorts_query_keys() {
        let url = Url::parse("http://example.com/?z=last&a=first").unwrap();
        assert_eq!(url.to_string(), "http://example.com/?a=first&z=last");
    }

    /// Scheme and host normalisation is reflected in `to_string()`.
    #[test]
    fn display_lowercases_scheme_and_host() {
        let url = Url::parse("HTTP://EXAMPLE.COM/").unwrap();
        assert_eq!(url.to_string(), "http://example.com/");
    }

    // -----------------------------------------------------------------------
    // Hash — consistent with PartialEq
    // -----------------------------------------------------------------------

    /// Equal URLs must have equal hashes.
    ///
    /// Business rule: `Hash` must be consistent with `PartialEq`. This enables
    /// use of `Url` as a key in `HashMap` / `HashSet`.
    #[test]
    fn equal_urls_have_equal_hashes() {
        let a = Url::parse("http://example.com/?foo=bar&spam=eggs").unwrap();
        let b = Url::parse("http://example.com/?spam=eggs&foo=bar").unwrap();
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    /// A URL hashes consistently across two calls (deterministic).
    #[test]
    fn url_hash_is_deterministic() {
        let url = Url::parse("https://example.com/path").unwrap();
        assert_eq!(hash_of(&url), hash_of(&url));
    }

    // -----------------------------------------------------------------------
    // PartialEq<&str>
    // -----------------------------------------------------------------------

    /// A `Url` must compare equal to a `&str` that represents the same canonical URL.
    ///
    /// Business rule: direct comparison with `&str` parses the string and
    /// compares canonical forms, enabling ergonomic assertions and matching.
    #[test]
    fn url_equals_equivalent_str() {
        let url = Url::parse("http://example.com/?foo=bar&spam=eggs").unwrap();
        assert!(url == "http://example.com/?foo=bar&spam=eggs");
    }

    /// A `Url` must compare equal to a `&str` even when query order differs.
    #[test]
    fn url_equals_str_with_different_query_order() {
        let url = Url::parse("http://example.com/?foo=bar&spam=eggs").unwrap();
        assert!(url == "http://example.com/?spam=eggs&foo=bar");
    }

    /// A `Url` must not compare equal to a `&str` with a different path.
    #[test]
    fn url_does_not_equal_str_with_different_path() {
        let url = Url::parse("https://example.com/foo").unwrap();
        assert!(url != "https://example.com/bar");
    }

    /// A `Url` compared to an invalid `&str` must return `false` (not panic).
    ///
    /// Business rule: invalid strings simply fail to match; they do not cause errors.
    #[test]
    fn url_does_not_equal_invalid_str() {
        let url = Url::parse("https://example.com/").unwrap();
        assert!(url != "not a url at all");
    }
}
