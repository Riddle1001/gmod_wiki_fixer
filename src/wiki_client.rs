use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub markup: String,
    #[serde(default)]
    pub html: String,
    #[serde(rename = "revisionId")]
    pub revision_id: Option<u64>,
}

impl WikiPage {
    fn is_missing(&self) -> bool {
        self.title == "Page Not Found"
            && self.revision_id == Some(0)
            && self.markup.contains("This page is missing.")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSummary {
    pub address: String,
    #[serde(rename = "updateCount", default)]
    pub update_count: u64,
    #[serde(rename = "viewCount", default)]
    pub view_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedPageEntry {
    #[serde(default)]
    origin: String,
    #[serde(default)]
    requested_address: String,
    update_count: Option<u64>,
    page: WikiPage,
}

pub struct WikiClient {
    client: Client,
    base_url: String,
    cache_dir: PathBuf,
    pagelist_cache: Option<HashMap<String, u64>>,
    reference_cache: HashMap<String, LinkResolution>,
    reference_pages: HashMap<String, WikiPage>,
    pub reference_requests: usize,
    pub page_views: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkResolution {
    Exists(String),
    Missing,
    Unverified(String),
}

impl Default for WikiClient {
    fn default() -> Self {
        Self::new("https://wiki.facepunch.com", Path::new("./.cache/pages"))
    }
}

#[cfg(test)]
pub(crate) fn test_server(
    routes: Vec<(&'static str, u16, String)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (expected, status, body) in routes {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "Expected request {}",
                            expected
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{}", error),
                }
            };
            // Accepted sockets inherit the listener's nonblocking mode on Windows.
            // A timeout alone does not make read wait for the HTTP request.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = [0u8; 8192];
            let length = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..length])
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_string();
            assert_eq!(request, expected);
            requests.push(request);
            let location = if status == 302 {
                format!("Location: {}\r\n", body)
            } else {
                String::new()
            };
            let body = if status == 302 { String::new() } else { body };
            write!(stream, "HTTP/1.1 {} Test\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, location, body.len(), body).unwrap();
        }
        requests
    });
    (format!("http://{}", address), handle)
}

impl WikiClient {
    pub fn new(base_url: &str, cache_dir: &Path) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) GmodWikiFixer/0.1.0")
            .build()
            .unwrap_or_else(|_| Client::new());

        let _ = fs::create_dir_all(cache_dir);

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            cache_dir: cache_dir.to_path_buf(),
            pagelist_cache: None,
            reference_cache: HashMap::new(),
            reference_pages: HashMap::new(),
            reference_requests: 0,
            page_views: BTreeMap::new(),
        }
    }

    /// Hashing bounds filename length and avoids Windows case folding. Identity is
    /// also checked in the payload, so even a hash collision is only a cache miss.
    pub(crate) fn cache_path_for(&self, page_name: &str) -> PathBuf {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.base_url.hash(&mut hash);
        page_name.hash(&mut hash);
        self.cache_dir
            .join(format!("v2-{:016x}.json", hash.finish()))
    }

    fn legacy_cache_path_for(&self, page_name: &str) -> PathBuf {
        let safe_name: String = page_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.cache_dir.join(format!("{}.json", safe_name))
    }

    /// Fetches the complete page index from `~pagelist?format=json` (1 single request for 4,400+ pages).
    pub fn fetch_pagelist(&mut self) -> Result<Vec<PageSummary>, String> {
        let url = format!("{}/gmod/~pagelist?format=json", self.base_url);
        let res = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("HTTP request to {} failed: {}", url, e))?;

        if !res.status().is_success() {
            return Err(format!("HTTP {} fetching ~pagelist", res.status()));
        }

        let summaries: Vec<PageSummary> = res
            .json()
            .map_err(|e| format!("Failed to parse ~pagelist JSON: {}", e))?;

        let mut map = HashMap::new();
        for item in &summaries {
            map.insert(item.address.clone(), item.update_count);
        }
        self.page_views = summaries
            .iter()
            .map(|item| (item.address.clone(), item.view_count))
            .collect();
        self.pagelist_cache = Some(map);
        self.reference_cache.clear();
        self.reference_pages.clear();

        Ok(summaries)
    }

    pub fn indexed_page_exists(&self, address: &str) -> bool {
        self.pagelist_cache
            .as_ref()
            .is_some_and(|pages| pages.contains_key(address))
    }

    #[cfg(test)]
    pub(crate) fn set_test_page_index(&mut self, addresses: &[&str]) {
        self.pagelist_cache = Some(
            addresses
                .iter()
                .map(|address| (address.to_string(), 0))
                .collect(),
        );
    }

    #[cfg(test)]
    pub(crate) fn cache_test_page(&self, address: &str, page: serde_json::Value) -> PathBuf {
        let path = self.cache_path_for(address);
        fs::write(
            &path,
            serde_json::json!({
                "origin": self.base_url, "requested_address": address,
                "update_count": 0, "page": page
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    /// Fetches the raw JSON for a page, using local disk caching validated with `updateCount`.
    pub fn fetch_page(&mut self, page_name: &str) -> Result<Option<WikiPage>, String> {
        // Every entry point has the same freshness policy. An index failure must
        // remain an operational error rather than silently trusting old pages.
        if self.pagelist_cache.is_none() {
            self.fetch_pagelist()?;
        }
        if let Some(page) = self.reference_pages.get(page_name) {
            return Ok(Some(page.clone()));
        }
        // An authoritative index absence must not be hidden by stale cached pages.
        if self
            .pagelist_cache
            .as_ref()
            .is_some_and(|pages| !pages.contains_key(page_name))
        {
            return Ok(None);
        }
        let cache_file = self.cache_path_for(page_name);

        // Check if we have an update_count from ~pagelist
        let expected_update_count = self
            .pagelist_cache
            .as_ref()
            .and_then(|m| m.get(page_name).copied());

        // Check local disk cache
        for (path, legacy) in [
            (&cache_file, false),
            (&self.legacy_cache_path_for(page_name), true),
        ] {
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(entry) = serde_json::from_str::<CachedPageEntry>(&data) {
                    let identity_matches = if legacy {
                        // Preserve existing official-wiki caches only when their
                        // canonical address proves which page the old key stored.
                        self.base_url == "https://wiki.facepunch.com"
                            && entry.page.address == page_name
                    } else {
                        entry.origin == self.base_url && entry.requested_address == page_name
                    };
                    if identity_matches
                        && !entry.page.is_missing()
                        && expected_update_count.is_some()
                        && entry.update_count == expected_update_count
                    {
                        return Ok(Some(entry.page));
                    }
                }
            }
        }

        let page = self.download_page(page_name)?;
        if let Some(page) = &page {
            let entry = CachedPageEntry {
                origin: self.base_url.clone(),
                requested_address: page_name.to_string(),
                update_count: expected_update_count,
                page: page.clone(),
            };
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = fs::write(cache_file, json);
            }
        }
        Ok(page)
    }

    fn download_page(&self, page_name: &str) -> Result<Option<WikiPage>, String> {
        let mut url = reqwest::Url::parse(&format!("{}/gmod/", self.base_url))
            .map_err(|error| error.to_string())?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "Invalid wiki base URL")?;
            segments.pop_if_empty();
            for segment in page_name.trim().split('/') {
                segments.push(segment);
            }
        }
        url.set_query(Some("format=json"));
        let mut res = self
            .client
            .get(url.clone())
            .send()
            .map_err(|e| format!("HTTP request to {} failed: {}", url, e))?;

        // Wiki redirects may discard ?format=json and return an HTML page.
        // Restore JSON on the canonical same-wiki URL before interpreting it.
        for _ in 0..8 {
            if res.url().origin() != url.origin() || !res.url().path().starts_with("/gmod/") {
                return Err("Page redirected outside the configured wiki".into());
            }
            if res
                .url()
                .query_pairs()
                .any(|(key, value)| key == "format" && value == "json")
            {
                break;
            }
            let mut canonical = res.url().clone();
            canonical.set_query(Some("format=json"));
            res = self
                .client
                .get(canonical)
                .send()
                .map_err(|error| format!("Canonical JSON request failed: {}", error))?;
        }
        if res.url().origin() != url.origin() || !res.url().path().starts_with("/gmod/") {
            return Err("Page redirected outside the configured wiki".into());
        }
        if !res
            .url()
            .query_pairs()
            .any(|(key, value)| key == "format" && value == "json")
        {
            return Err("Canonical JSON redirect depth exceeded".into());
        }
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !res.status().is_success() {
            return Err(format!(
                "HTTP error {} fetching page {}",
                res.status(),
                page_name
            ));
        }

        let resolved_address = res
            .url()
            .path()
            .strip_prefix("/gmod/")
            .map(crate::wiki_markup::decode_uri_path);
        let mut page: WikiPage = res
            .json()
            .map_err(|e| format!("Failed to parse JSON from {}: {}", url, e))?;

        // The live wiki reports missing pages with HTTP 200 and revisionId 0.
        // Require its explicit missing-page payload, not merely an empty page.
        if page.is_missing() {
            return Ok(None);
        }
        if page.address.is_empty() {
            page.address = resolved_address.unwrap_or_else(|| page_name.to_string());
        }
        Ok(Some(page))
    }

    /// Index hits and canonical spelling aliases are cheap. Index misses receive
    /// fresh GETs so HTTP redirects and unlisted live pages are not called broken.
    pub fn resolve_reference(&mut self, address: &str) -> LinkResolution {
        if let Some(result) = self.reference_cache.get(address) {
            return result.clone();
        }
        if self.pagelist_cache.is_none() {
            if let Err(error) = self.fetch_pagelist() {
                return LinkResolution::Unverified(error);
            }
        }
        let mut current = address.to_string();
        let mut seen = std::collections::HashSet::new();
        let result = loop {
            if !seen.insert(current.clone()) || seen.len() > 8 {
                break LinkResolution::Unverified(
                    "redirect cycle or excessive redirect depth".into(),
                );
            }
            if self.indexed_page_exists(&current) {
                break LinkResolution::Exists(current);
            }
            let (parent, name) = crate::diff_engine::parse_address_parts(&current);
            let mut aliases = vec![current.replace(' ', "_")];
            if let Some(parent) = parent {
                aliases.push(format!("{}.{}", parent, name));
                aliases.push(format!("{}:{}", parent, name));
                if parent == "Global" {
                    aliases.push(name.clone());
                }
                if matches!(parent.as_str(), "Panel" | "PANEL") {
                    let parent = if parent == "Panel" { "PANEL" } else { "Panel" };
                    aliases.push(format!("{}:{}", parent, name));
                    aliases.push(format!("{}.{}", parent, name));
                }
            } else {
                aliases.push(format!("Global.{}", name));
            }
            if let Some(alias) = aliases
                .into_iter()
                .find(|alias| self.indexed_page_exists(alias))
            {
                break LinkResolution::Exists(alias);
            }
            self.reference_requests += 1;
            match self.download_page(&current) {
                Ok(Some(page)) => {
                    let redirect = crate::wiki_markup::blocks(&page.markup, "redirect")
                        .first()
                        .and_then(|block| {
                            block.tag.attrs.get("to").cloned().or_else(|| {
                                crate::audit_checks::normalize_reference(block.inner.trim())
                            })
                        })
                        .or_else(|| {
                            let markup = page.markup.trim();
                            if !markup.to_ascii_lowercase().starts_with("#redirect") {
                                return None;
                            }
                            let start = markup.find("[[")? + 2;
                            let end = markup[start..].find("]]")? + start;
                            crate::audit_checks::normalize_reference(&markup[start..end])
                        });
                    if let Some(redirect) = redirect {
                        current = redirect;
                        continue;
                    }
                    let canonical = if page.address.is_empty() {
                        current.clone()
                    } else {
                        page.address.clone()
                    };
                    self.reference_pages
                        .insert(address.to_string(), page.clone());
                    self.reference_pages.insert(canonical.clone(), page);
                    break LinkResolution::Exists(canonical);
                }
                Ok(None) => break LinkResolution::Missing,
                Err(error) => break LinkResolution::Unverified(error),
            }
        };
        self.reference_cache
            .insert(address.to_string(), result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_official_cache_is_preserved_when_identity_and_version_match() {
        let dir = std::env::temp_dir().join(format!("legacy-cache-{}", std::process::id()));
        let mut client = WikiClient::new("https://wiki.facepunch.com", &dir);
        client.set_test_page_index(&["Global.Probe"]);
        let path = dir.join("Global.Probe.json");
        fs::write(
            &path,
            serde_json::json!({"update_count":0,"page":{
                "address":"Global.Probe","title":"Existing cached page","markup":"old cache format"
            }})
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            client.fetch_page("Global.Probe").unwrap().unwrap().title,
            "Existing cached page"
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn redirects_outside_wiki_are_rejected_even_when_json_query_is_preserved() {
        let (base, server) = test_server(vec![
            (
                "/gmod/Legacy?format=json",
                302,
                "/outside?format=json".into(),
            ),
            (
                "/outside?format=json",
                200,
                r#"{"address":"Unrelated","markup":"unrelated"}"#.into(),
            ),
        ]);
        let dir = std::env::temp_dir().join(format!("redirect-boundary-{}", std::process::id()));
        let mut client = WikiClient::new(&base, &dir);
        client.set_test_page_index(&["Legacy"]);
        assert!(client.fetch_page("Legacy").is_err());
        assert_eq!(server.join().unwrap().len(), 2);
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn soft_404_is_missing_and_redirects_that_drop_json_are_refetched() {
        let missing = serde_json::json!({"title":"Page Not Found","address":"Absent","revisionId":0,"markup":"<title>Page Not Found</title>\nThis page is missing."}).to_string();
        let live = serde_json::json!({"title":"Canonical","address":"Canonical","revisionId":10,"markup":"Live page"}).to_string();
        let empty =
            serde_json::json!({"title":"Empty","address":"Empty","revisionId":0,"markup":""})
                .to_string();
        let (base, server) = test_server(vec![
            ("/gmod/Absent?format=json", 200, missing),
            ("/gmod/Legacy?format=json", 302, "/gmod/Canonical".into()),
            ("/gmod/Canonical", 200, "<html>Live page</html>".into()),
            ("/gmod/Canonical?format=json", 200, live),
            ("/gmod/Empty?format=json", 200, empty),
        ]);
        let cache = std::env::temp_dir().join(format!("soft404-http-{}", std::process::id()));
        let mut client = WikiClient::new(&base, &cache);
        client.set_test_page_index(&[]);
        assert_eq!(client.resolve_reference("Absent"), LinkResolution::Missing);
        assert_eq!(
            client.resolve_reference("Legacy"),
            LinkResolution::Exists("Canonical".into())
        );
        assert_eq!(
            client.resolve_reference("Empty"),
            LinkResolution::Exists("Empty".into())
        );
        assert_eq!(server.join().unwrap().len(), 5);
        fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn redirect_and_unlisted_pages_are_resolved_once_and_errors_are_not_missing() {
        let body = serde_json::json!({"address":"New", "title":"New", "markup":"<function name='New'></function>"}).to_string();
        let (base, server) = test_server(vec![
            ("/gmod/Old?format=json", 302, "/gmod/New?format=json".into()),
            ("/gmod/New?format=json", 200, body),
            ("/gmod/Missing?format=json", 404, "{}".into()),
            ("/gmod/Error?format=json", 500, "{}".into()),
        ]);
        let cache = std::env::temp_dir().join(format!("link-http-{}", std::process::id()));
        let mut client = WikiClient::new(&base, &cache);
        client.set_test_page_index(&[]);
        assert_eq!(
            client.resolve_reference("Old"),
            LinkResolution::Exists("New".into())
        );
        assert_eq!(
            client.resolve_reference("Old"),
            LinkResolution::Exists("New".into())
        );
        assert!(client.fetch_page("New").unwrap().is_some());
        assert_eq!(client.resolve_reference("Missing"), LinkResolution::Missing);
        assert!(matches!(
            client.resolve_reference("Error"),
            LinkResolution::Unverified(_)
        ));
        assert_eq!(client.reference_requests, 3);
        assert_eq!(server.join().unwrap().len(), 4);
        fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn indexed_spelling_aliases_require_no_network_requests() {
        let cache = std::env::temp_dir().join(format!("link-index-{}", std::process::id()));
        let mut client = WikiClient::new("http://127.0.0.1:1", &cache);
        client.set_test_page_index(&["Panel:SetPos", "Global.Color"]);
        assert_eq!(
            client.resolve_reference("PANEL.SetPos"),
            LinkResolution::Exists("Panel:SetPos".into())
        );
        assert_eq!(
            client.resolve_reference("Color"),
            LinkResolution::Exists("Global.Color".into())
        );
        assert_eq!(client.reference_requests, 0);
        fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn index_absence_overrides_stale_cache_without_http() {
        let dir = std::env::temp_dir().join(format!("wiki-fixer-test-{}", std::process::id()));
        let mut client = WikiClient::new("http://127.0.0.1:1", &dir);
        let address = "Global.Removed";
        let cache = client.cache_path_for(address);
        let page = WikiPage {
            title: "Removed".into(),
            address: address.into(),
            markup: "old".into(),
            html: String::new(),
            revision_id: None,
        };
        fs::write(
            &cache,
            serde_json::to_string(&CachedPageEntry {
                origin: client.base_url.clone(),
                requested_address: address.into(),
                update_count: Some(0),
                page,
            })
            .unwrap(),
        )
        .unwrap();
        client.pagelist_cache = Some(HashMap::new());
        assert!(client.fetch_page(address).unwrap().is_none());
        fs::remove_file(cache).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
