//! Turns whatever was typed in the address field into a URL: a real URL, a bare domain,
//! or a search.

use url::Url;

pub const SEARCH_URL: &str = "https://duckduckgo.com/?q=";

/// Schemes we load as typed. Anything else that happens to parse (`foo:bar`) is a search.
const KNOWN_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "data",
    "about",
    "drift",
    "view-source",
];

pub fn resolve(input: &str) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    if let Ok(url) = Url::parse(input)
        && KNOWN_SCHEMES.contains(&url.scheme())
    {
        return Some(url);
    }

    if looks_like_host(input) {
        // hmm, localhost and plain IPs rarely speak TLS in dev setups, so default those to http
        let scheme = if is_local(input) { "http" } else { "https" };
        if let Ok(url) = Url::parse(&format!("{scheme}://{input}")) {
            return Some(url);
        }
    }

    let query: String = url::form_urlencoded::byte_serialize(input.as_bytes()).collect();
    Url::parse(&format!("{SEARCH_URL}{query}")).ok()
}

fn looks_like_host(input: &str) -> bool {
    if input.contains(char::is_whitespace) {
        return false;
    }
    let host = input
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once(':')
        .map(|(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                ""
            }
        })
        .unwrap_or(input.split(['/', '?', '#']).next().unwrap_or_default());
    if host.is_empty() {
        return false;
    }
    host == "localhost"
        || (host.contains('.')
            && !host.starts_with('.')
            && !host.ends_with('.')
            && host
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-'))
}

fn is_local(input: &str) -> bool {
    input.starts_with("localhost")
        || input.starts_with("127.")
        || input.starts_with("192.168.")
        || input.starts_with("10.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        resolve(s).map(|u| u.to_string()).unwrap_or_default()
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(r("https://servo.org/"), "https://servo.org/");
        assert_eq!(
            r("  http://example.com/a?b=c  "),
            "http://example.com/a?b=c"
        );
        assert_eq!(r("about:blank"), "about:blank");
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(r("servo.org"), "https://servo.org/");
        assert_eq!(
            r("github.com/servo/servo"),
            "https://github.com/servo/servo"
        );
        assert_eq!(r("example.com:8443/x"), "https://example.com:8443/x");
    }

    #[test]
    fn local_hosts_get_http() {
        assert_eq!(r("localhost:3000"), "http://localhost:3000/");
        assert_eq!(r("127.0.0.1:8080"), "http://127.0.0.1:8080/");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(
            r("rust borrow checker"),
            format!("{SEARCH_URL}rust+borrow+checker")
        );
        assert_eq!(r("foo:bar"), format!("{SEARCH_URL}foo%3Abar"));
        assert_eq!(r("servo"), format!("{SEARCH_URL}servo"));
        assert_eq!(r("c++"), format!("{SEARCH_URL}c%2B%2B"));
    }

    #[test]
    fn empty_is_nothing() {
        assert!(resolve("   ").is_none());
    }
}
