//! GitHub metadata and verified package distribution.
use crate::{catalog, conformance, error::ensure, store, Error, Result};
use reqwest::{blocking::Client, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
    process::Command,
    time::Duration,
};

const MAX: u64 = 1_048_576;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    pub description: String,
    pub publisher: String,
    pub repository: String,
    pub asset_id: u64,
    pub sha256: String,
    pub permissions: Vec<String>,
    pub hosting: String,
    pub trust: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosting_details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    /// Namespace -> GitHub account/organization that owns its package repositories.
    pub publishers: BTreeMap<String, String>,
    pub apps: Vec<Entry>,
}

fn repository(s: &str) -> bool {
    let parts: Vec<_> = s.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
}

pub fn validate(index: &Index, base: Option<&Index>) -> Result<()> {
    ensure(index.format == 1, "registry", "Unsupported index format")?;
    ensure(
        index.apps.len() <= 100,
        "registry",
        "Prototype registry is limited to 100 package versions",
    )?;
    for (namespace, owner) in &index.publishers {
        ensure(
            catalog::app_name(&format!("{namespace}/app")) && repository(&format!("{owner}/repo")),
            "registry",
            "Invalid publisher ownership",
        )?;
    }
    let mut seen = BTreeSet::new();
    for e in &index.apps {
        if let Some(name) = &e.display_name {
            ensure(
                !name.trim().is_empty()
                    && name.chars().count() <= 120
                    && !name.chars().any(char::is_control),
                "registry",
                "Invalid display_name",
            )?;
        }
        ensure(
            catalog::app_name(&e.name) && e.name.split('/').next() == Some(e.publisher.as_str()),
            "registry",
            "App namespace must match publisher",
        )?;
        catalog::version(&e.version)?;
        ensure(
            seen.insert((&e.name, &e.version)),
            "registry",
            "Duplicate package version",
        )?;
        ensure(
            repository(&e.repository) && e.asset_id > 0,
            "registry",
            "Invalid release asset location",
        )?;
        let owner = index
            .publishers
            .get(&e.publisher)
            .ok_or_else(|| Error::new("registry", "Unregistered publisher namespace"))?;
        ensure(
            e.repository
                .split('/')
                .next()
                .is_some_and(|s| s.eq_ignore_ascii_case(owner)),
            "registry",
            "Package repository must belong to namespace owner",
        )?;
        ensure(
            e.sha256.len() == 64
                && e.sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "registry",
            "Expected lowercase SHA-256 of release asset bytes",
        )?;
        ensure(
            matches!(e.trust.as_str(), "Unverified" | "Community"),
            "registry",
            "Verified/certified badges require a future verification authority",
        )?;
        ensure(
            !e.description.trim().is_empty()
                && ["local", "self-hosted", "remote"].contains(&e.hosting.as_str()),
            "registry",
            "Invalid package metadata",
        )?;
        let contract =
            json!({"execution":e.execution.clone().unwrap_or(json!({"driver":"declarative"}))});
        crate::container::validate_execution(&contract)?;
        let container = crate::container::enabled(&contract);
        ensure(
            crate::services::enabled(&contract) == e.permissions.iter().any(|s| s == "service.run"),
            "registry",
            "Persistent service execution must disclose service.run",
        )?;
        ensure(
            contract["execution"]["calls"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
                == e.permissions.iter().any(|s| s == "app.call"),
            "registry",
            "Peer calls must disclose app.call",
        )?;
        ensure(
            container == e.permissions.iter().any(|s| s == "container.execute"),
            "registry",
            "Container execution must be disclosed in registry metadata",
        )?;
        ensure(
            !container
                || (e.hosting == "local"
                    && contract["execution"]["image"]
                        .as_str()
                        .unwrap()
                        .contains("@sha256:")),
            "registry",
            "Published container apps require local hosting and a distributable image digest",
        )?;
    }
    if let Some(old) = base {
        for e in &old.apps {
            ensure(
                index.apps.iter().any(|n| n == e),
                "immutable_version",
                "Published entries cannot be removed or changed; publish a new version",
            )?;
        }
        for (namespace, owner) in &old.publishers {
            ensure(
                index.publishers.get(namespace) == Some(owner),
                "registry",
                "Existing namespace ownership cannot be changed",
            )?;
        }
    }
    Ok(())
}

pub fn read_index(path: &Path) -> Result<Index> {
    ensure(
        std::fs::metadata(path)?.len() <= MAX,
        "registry",
        "Index exceeds 1 MiB",
    )?;
    let index = serde_json::from_slice(&std::fs::read(path)?)?;
    validate(&index, None)?;
    Ok(index)
}

fn token(anonymous: bool) -> Option<String> {
    if anonymous {
        return None;
    }
    for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Some(value.trim().into());
            }
        }
    }
    // Credentials remain in memory; never print or persist them. Support older gh.
    for args in [
        vec!["auth", "token", "--hostname", "github.com"],
        vec!["config", "get", "oauth_token", "--host", "github.com"],
    ] {
        if let Ok(output) = Command::new("gh").args(args).output() {
            if output.status.success() {
                if let Ok(value) = String::from_utf8(output.stdout) {
                    if !value.trim().is_empty() {
                        return Some(value.trim().into());
                    }
                }
            }
        }
    }
    None
}

struct Github {
    client: Client,
    token: Option<String>,
}
impl Github {
    fn new(anonymous: bool) -> Result<Self> {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .user_agent("rhyven-registry/1")
            .build()
            .map_err(|_| Error::new("network", "Could not initialize GitHub client"))?;
        Ok(Self {
            client,
            token: token(anonymous),
        })
    }
    fn get(&self, mut url: Url, accept: &str) -> Result<Vec<u8>> {
        for _ in 0..4 {
            ensure(
                allowed_url(&url),
                "registry",
                "Only GitHub HTTPS endpoints are allowed",
            )?;
            let mut req = self.client.get(url.clone()).header("Accept", accept);
            if url.host_str() == Some("api.github.com") {
                if let Some(t) = &self.token {
                    req = req.bearer_auth(t);
                }
            }
            let response = req
                .send()
                .map_err(|_| Error::new("network", "GitHub request failed; check connectivity"))?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|s| s.to_str().ok())
                    .ok_or_else(|| Error::new("network", "GitHub redirect has no location"))?;
                url = url
                    .join(location)
                    .map_err(|_| Error::new("network", "Invalid redirect"))?;
                continue;
            }
            ensure(response.status().is_success(), "network", format!("GitHub returned HTTP {}; private repositories need gh auth login or GH_TOKEN with contents access", response.status().as_u16()))?;
            let mut bytes = Vec::new();
            response.take(MAX + 1).read_to_end(&mut bytes)?;
            ensure(
                bytes.len() as u64 <= MAX,
                "registry",
                "GitHub response exceeds 1 MiB",
            )?;
            return Ok(bytes);
        }
        Err(Error::new("network", "Too many GitHub redirects"))
    }
    fn package(&self, e: &Entry) -> Result<Value> {
        let url = Url::parse(&format!(
            "https://api.github.com/repos/{}/releases/assets/{}",
            e.repository, e.asset_id
        ))
        .unwrap();
        verify_package(e, &self.get(url, "application/octet-stream")?)
    }
}
fn allowed_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
pub fn verify_package(e: &Entry, bytes: &[u8]) -> Result<Value> {
    ensure(
        bytes.len() as u64 <= MAX && format!("{:x}", Sha256::digest(bytes)) == e.sha256,
        "integrity",
        "Release asset SHA-256 mismatch",
    )?;
    let p: Value = serde_json::from_slice(bytes)?;
    catalog::validate(&p)?;
    ensure(
        p.get("execution") == e.execution.as_ref(),
        "integrity",
        "Execution differs from registry disclosures",
    )?;
    ensure(
        p["name"] == e.name
            && p.get("display_name").and_then(Value::as_str) == e.display_name.as_deref()
            && p["version"] == e.version
            && p["publisher"] == e.publisher
            && p["description"] == e.description
            && p["permissions"] == json!(e.permissions)
            && p["hosting"]["mode"] == e.hosting,
        "integrity",
        "Index metadata does not match package",
    )?;
    if let Some(details) = &e.hosting_details {
        ensure(
            &p["hosting"] == details,
            "integrity",
            "Hosting disclosures differ from index",
        )?;
    }
    Ok(p)
}

pub fn entry(path: &Path, repo: &str, asset_id: u64) -> Result<Value> {
    let p = catalog::read(path)?;
    ensure(
        path.is_file() && repository(repo) && asset_id > 0,
        "registry",
        "Provide a packaged file, owner/repo and numeric release asset ID",
    )?;
    let e = Entry {
        name: p["name"].as_str().unwrap().into(),
        display_name: p["display_name"].as_str().map(str::to_owned),
        version: p["version"].as_str().unwrap().into(),
        description: p["description"].as_str().unwrap().into(),
        publisher: p["publisher"].as_str().unwrap().into(),
        repository: repo.into(),
        asset_id,
        sha256: format!("{:x}", Sha256::digest(std::fs::read(path)?)),
        permissions: serde_json::from_value(p["permissions"].clone())?,
        hosting: p["hosting"]["mode"].as_str().unwrap().into(),
        trust: "Unverified".into(),
        hosting_details: (p["hosting"]["mode"] != "local").then(|| p["hosting"].clone()),
        execution: p.get("execution").cloned(),
    };
    ensure(
        !crate::container::enabled(&p)
            || p["execution"]["image"]
                .as_str()
                .unwrap()
                .contains("@sha256:"),
        "registry",
        "Push your container image and use repository@sha256:digest before publishing",
    )?;
    Ok(serde_json::to_value(e)?)
}

/// Validate registry submissions with the same package validator and isolated tests.
pub fn check(path: &Path, base: Option<&Path>, anonymous: bool) -> Result<Value> {
    let index = read_index(path)?;
    let base = base.map(read_index).transpose()?;
    validate(&index, base.as_ref())?;
    let client = Github::new(anonymous)?;
    for e in &index.apps {
        let p = client.package(e)?;
        if e.hosting == "local" && !crate::container::enabled(&p) {
            conformance::run(&p)?;
        }
    }
    Ok(
        json!({"valid":true,"packages":index.apps.len(),"trust":"Unverified","container_tests":"not executed; publisher code requires explicit isolated testing"}),
    )
}

/// Download the complete small V1 registry, then commit one atomic offline cache.
pub fn sync(root: &Path, repo: &str, branch: &str, anonymous: bool) -> Result<Value> {
    ensure(
        repository(repo) && !branch.is_empty(),
        "registry",
        "Expected owner/repo and a ref",
    )?;
    let client = Github::new(anonymous)?;
    let mut url = Url::parse(&format!(
        "https://api.github.com/repos/{repo}/contents/index.json"
    ))
    .unwrap();
    url.query_pairs_mut().append_pair("ref", branch);
    let index: Index =
        serde_json::from_slice(&client.get(url, "application/vnd.github.raw+json")?)?;
    cache_index(root, repo, branch, index, |e| client.package(e))
}

fn cache_index(
    root: &Path,
    repo: &str,
    branch: &str,
    index: Index,
    mut download: impl FnMut(&Entry) -> Result<Value>,
) -> Result<Value> {
    let cache_path = crate::collections::registry_dir(root)?.join("github-registry.json");
    let previous = if cache_path.exists() {
        Some(read_cache(root)?)
    } else {
        None
    };
    if let Some(ref old) = previous {
        ensure(
            old["repository"] == repo,
            "registry",
            "Workspace already uses another registry; use a separate workspace",
        )?;
        let old_index: Index = serde_json::from_value(old["index"].clone())?;
        validate(&index, Some(&old_index))?;
    } else {
        validate(&index, None)?;
    }
    let local = catalog::list(root)?;
    let mut packages = Vec::new();
    for e in &index.apps {
        let p = download(e)?;
        for old in &local {
            if old["name"] == e.name && old["version"] == e.version {
                ensure(
                    old == &p,
                    "immutable_version",
                    "Registry conflicts with an existing package version",
                )?;
            }
        }
        packages
            .push(json!({"manifest":crate::collections::save(root, &p)?,"digest":store::hash(&p)}));
    }
    std::fs::create_dir_all(crate::collections::registry_dir(root)?)?;
    store::write(
        &cache_path,
        &json!({"repository":repo,"ref":branch,"index":index,"packages":packages}),
    )?;
    Ok(
        json!({"synced":repo,"ref":branch,"packages":packages.len(),"cache":cache_path,"installed":false}),
    )
}

fn read_cache(root: &Path) -> Result<Value> {
    let path = crate::collections::registry_dir(root)?.join("github-registry.json");
    ensure(
        std::fs::metadata(&path)?.len() <= 110 * MAX,
        "registry",
        "Registry cache exceeds size limit",
    )?;
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn cached(root: &Path) -> Result<Vec<Value>> {
    if !crate::collections::registry_dir(root)?
        .join("github-registry.json")
        .exists()
    {
        return Ok(vec![]);
    }
    let cache = read_cache(root)?;
    let index: Index = serde_json::from_value(cache["index"].clone())?;
    validate(&index, None)?;
    let packages = cache["packages"]
        .as_array()
        .ok_or_else(|| Error::new("registry", "Invalid cache"))?;
    ensure(
        packages.len() == index.apps.len(),
        "integrity",
        "Incomplete registry cache",
    )?;
    packages
        .iter()
        .zip(&index.apps)
        .map(|(item, e)| {
            let p = crate::collections::load(root, &item["manifest"])?;
            catalog::validate(&p)?;
            ensure(
                item["digest"] == store::hash(&p)
                    && p["name"] == e.name
                    && p["version"] == e.version,
                "integrity",
                "Registry cache package mismatch",
            )?;
            Ok(p)
        })
        .collect()
}

/// Metadata-only refresh: no calls to the release-assets API.
pub fn refresh(root: &Path, repo: &str, branch: &str, anonymous: bool) -> Result<Value> {
    ensure(
        repository(repo) && !branch.is_empty(),
        "registry",
        "Expected owner/repo and ref",
    )?;
    let client = Github::new(anonymous)?;
    let mut url = Url::parse(&format!(
        "https://api.github.com/repos/{repo}/contents/index.json"
    ))
    .unwrap();
    url.query_pairs_mut().append_pair("ref", branch);
    let index: Index =
        serde_json::from_slice(&client.get(url, "application/vnd.github.raw+json")?)?;
    let previous = metadata(root)?;
    if let Some(old) = &previous {
        ensure(
            old["repository"] == repo,
            "registry",
            "Workspace already uses another registry",
        )?;
        validate(&index, Some(&serde_json::from_value(old["index"].clone())?))?;
    } else {
        validate(&index, None)?;
    }
    let checked_at = crate::marketplace::now();
    let mut stars = serde_json::Map::new();
    for entry in &index.apps {
        if stars.contains_key(&entry.repository) {
            continue;
        }
        let count = client
            .get(
                Url::parse(&format!(
                    "https://api.github.com/repos/{}",
                    entry.repository
                ))
                .unwrap(),
                "application/vnd.github+json",
            )
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v["stargazers_count"].as_u64());
        stars.insert(entry.repository.clone(), json!({"count":count,"checked_at":checked_at,"source":"GitHub repository","available":count.is_some()}));
    }
    let value = json!({"repository":repo,"ref":branch,"anonymous":anonymous,"index":index,"stars":stars,"checked_at":checked_at});
    std::fs::create_dir_all(crate::collections::registry_dir(root)?)?;
    store::write(
        &crate::collections::registry_dir(root)?.join("market-metadata.json"),
        &value,
    )?;
    Ok(
        json!({"refreshed":repo,"listings":index.apps.len(),"package_downloads":0,"checked_at":checked_at}),
    )
}

pub fn metadata(root: &Path) -> Result<Option<Value>> {
    let path = crate::collections::registry_dir(root)?.join("market-metadata.json");
    let value = if path.exists() {
        ensure(
            std::fs::metadata(&path)?.len() <= 2 * MAX,
            "registry",
            "Metadata cache too large",
        )?;
        serde_json::from_slice(&std::fs::read(path)?)?
    } else if crate::collections::registry_dir(root)?
        .join("github-registry.json")
        .exists()
    {
        let mut old = read_cache(root)?;
        old.as_object_mut().unwrap().remove("packages");
        old
    } else {
        return Ok(None);
    };
    let index: Index = serde_json::from_value(value["index"].clone())?;
    validate(&index, None)?;
    ensure(
        repository(value["repository"].as_str().unwrap_or("")),
        "registry",
        "Invalid registry repository",
    )?;
    Ok(Some(value))
}

pub fn download(entry: &Entry, anonymous: bool) -> Result<Value> {
    Github::new(anonymous)?.package(entry)
}

/// Reuse verified releases across collections, only after the caller obtains consent.
pub fn download_cached(root: &Path, entry: &Entry, anonymous: bool) -> Result<Value> {
    cached_download_with(root, entry, || download(entry, anonymous))
}
fn cached_download_with(
    root: &Path,
    entry: &Entry,
    fetch: impl FnOnce() -> Result<Value>,
) -> Result<Value> {
    let Some(home) = crate::collections::home_for(root)? else {
        return fetch();
    };
    let dir = home.join("packages/releases");
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("cache.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let entry_value = serde_json::to_value(entry)?;
    let path = dir.join(format!("{}.json", store::hash(&entry_value)));
    if path.exists() {
        ensure(
            std::fs::metadata(&path)?.len() <= MAX,
            "integrity",
            "Cached release metadata exceeds size limit",
        )?;
        let cache: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure(
            cache["entry"] == entry_value,
            "integrity",
            "Cached release metadata mismatch",
        )?;
        return crate::collections::load(root, &cache["package"]);
    }
    let package = fetch()?;
    let reference = crate::collections::save(root, &package)?;
    store::write(&path, &json!({"entry":entry_value,"package":reference}))?;
    Ok(package)
}

/// Resolve an install from metadata without fetching publisher assets before consent.
pub fn resolve_install(root: &Path, selector: &str, accepted: bool) -> Result<Value> {
    if Path::new(selector).exists() {
        return catalog::resolve(root, selector);
    }
    let (name, version) = selector
        .split_once('@')
        .map_or((selector, None), |(n, v)| (n, Some(v)));
    let local = catalog::resolve(root, selector);
    if let Some(meta) = metadata(root)? {
        let index: Index = serde_json::from_value(meta["index"].clone())?;
        let entry = index
            .apps
            .iter()
            .filter(|e| e.name == name && version.is_none_or(|v| e.version == v))
            .max_by_key(|e| catalog::version(&e.version).unwrap());
        if let Some(entry) = entry {
            let remote_version = catalog::version(&entry.version)?;
            let prefer_remote = local.as_ref().map_or(true, |p| {
                catalog::version(p["version"].as_str().unwrap()).is_ok_and(|v| remote_version >= v)
            });
            if prefer_remote {
                ensure(
                    accepted,
                    "permission_review_required",
                    format!(
                        "Review listing permissions and execution before installing into {}: {}",
                        crate::collections::scope(root)?,
                        serde_json::to_string(entry)?
                    ),
                )?;
                return download_cached(root, entry, meta["anonymous"].as_bool().unwrap_or(false));
            }
        }
    }
    local
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collections_reuse_verified_release_and_detect_corruption() {
        let d = tempfile::tempdir().unwrap();
        let a = crate::collections::create(d.path(), "global").unwrap();
        let b = crate::collections::create(d.path(), "project").unwrap();
        let (index, bytes) = fixture();
        let p = cached_download_with(&a, &index.apps[0], || {
            verify_package(&index.apps[0], &bytes)
        })
        .unwrap();
        assert_eq!(
            cached_download_with(&b, &index.apps[0], || panic!("duplicate download")).unwrap(),
            p
        );
        std::fs::write(
            d.path()
                .join("packages/sha256")
                .join(format!("{}.json", store::hash(&p))),
            b"{}",
        )
        .unwrap();
        assert!(
            cached_download_with(&b, &index.apps[0], || panic!("corrupt cache must fail")).is_err()
        );
    }
    #[test]
    fn cli_metadata_install_requires_consent_before_asset_fetch() {
        let home = tempfile::tempdir().unwrap();
        let root = crate::collections::create(home.path(), "global").unwrap();
        let (index, bytes) = fixture();
        let dir = crate::collections::registry_dir(&root).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        store::write(
            &dir.join("market-metadata.json"),
            &json!({"repository":"example-publisher/apps","index":index,"anonymous":true}),
        )
        .unwrap();
        assert_eq!(
            resolve_install(&root, &index.apps[0].name, false)
                .unwrap_err()
                .code,
            "permission_review_required"
        );
        assert!(!home.path().join("packages/releases").exists());
        let expected = cached_download_with(&root, &index.apps[0], || {
            verify_package(&index.apps[0], &bytes)
        })
        .unwrap();
        assert_eq!(
            resolve_install(&root, &index.apps[0].name, true).unwrap(),
            expected
        );
    }
    fn fixture() -> (Index, Vec<u8>) {
        let p = catalog::bundled().remove(0);
        let bytes = serde_json::to_vec_pretty(&p).unwrap();
        let e: Entry = serde_json::from_value(json!({"name":p["name"],"display_name":p["display_name"],"version":p["version"],"publisher":p["publisher"],"description":p["description"],"permissions":p["permissions"],"hosting":"local","trust":"Community","repository":"example-publisher/apps","asset_id":42,"sha256":format!("{:x}", Sha256::digest(&bytes))})).unwrap();
        (
            Index {
                format: 1,
                publishers: BTreeMap::from([("rhyven".into(), "example-publisher".into())]),
                apps: vec![e],
            },
            bytes,
        )
    }
    #[test]
    fn ownership_immutability_metadata_and_hash_fail_closed() {
        let (index, bytes) = fixture();
        validate(&index, None).unwrap();
        verify_package(&index.apps[0], &bytes).unwrap();
        let mut bad = index.clone();
        bad.apps[0].repository = "impostor/apps".into();
        assert!(validate(&bad, None).is_err());
        bad = index.clone();
        bad.apps[0].trust = "Verified".into();
        assert!(validate(&bad, None).is_err());
        bad = index.clone();
        bad.apps[0].trust = "Unverified".into();
        validate(&bad, None).unwrap();
        bad.apps[0].display_name = Some("Different advertised app".into());
        assert!(verify_package(&bad.apps[0], &bytes).is_err());
        bad = index.clone();
        bad.apps[0].asset_id += 1;
        assert!(validate(&bad, Some(&index)).is_err());
        assert!(verify_package(&index.apps[0], b"{}").is_err());
        bad = index.clone();
        bad.apps[0].permissions.push("shell.execute".into());
        assert!(verify_package(&bad.apps[0], &bytes).is_err());
        bad = index.clone();
        bad.apps.clear();
        assert!(validate(&bad, Some(&index)).is_err());
        for url in [
            "http://api.github.com",
            "https://api.github.com.evil.test",
            "https://evil.test",
            "https://token@api.github.com",
            "https://api.github.com:444",
        ] {
            assert!(!allowed_url(&Url::parse(url).unwrap()));
        }
        assert!(allowed_url(
            &Url::parse("https://release-assets.githubusercontent.com/asset?sig=test").unwrap()
        ));
    }
    #[test]
    fn synced_upgrade_resolves_new_version_and_keeps_records() {
        let d = tempfile::tempdir().unwrap();
        let (mut index, bytes) = fixture();
        cache_index(
            d.path(),
            "example-publisher/registry",
            "main",
            index.clone(),
            |e| verify_package(e, &bytes),
        )
        .unwrap();
        let runtime = crate::Runtime::new(d.path(), "test").unwrap();
        let name = index.apps[0].name.clone();
        runtime
            .install(&catalog::resolve(d.path(), &name).unwrap(), true, false)
            .unwrap();
        let record = runtime
            .call(
                "create",
                json!({"app":name,"object":"task","data":{"title":"Keep across upgrade"}}),
            )
            .unwrap();
        let mut package: Value = serde_json::from_slice(&bytes).unwrap();
        package["version"] = json!("99.0.0");
        package["objects"]["task"]["schema"]["properties"]["tag"] = json!({"type":"string"});
        let new_bytes = serde_json::to_vec_pretty(&package).unwrap();
        let mut entry = index.apps[0].clone();
        entry.version = "99.0.0".into();
        entry.asset_id += 1;
        entry.sha256 = format!("{:x}", Sha256::digest(&new_bytes));
        index.apps.push(entry);
        cache_index(d.path(), "example-publisher/registry", "main", index, |e| {
            verify_package(
                e,
                if e.version == "99.0.0" {
                    &new_bytes
                } else {
                    &bytes
                },
            )
        })
        .unwrap();
        let latest = catalog::resolve(d.path(), &name).unwrap();
        assert_eq!(latest["version"], "99.0.0");
        runtime.install(&latest, true, true).unwrap();
        assert_eq!(
            runtime
                .call("get", json!({"app":name,"object":"task","id":record["id"]}))
                .unwrap(),
            record
        );
    }
    #[test]
    fn failed_sync_preserves_cache_and_sync_never_installs() {
        let d = tempfile::tempdir().unwrap();
        let (index, bytes) = fixture();
        cache_index(
            d.path(),
            "example-publisher/registry",
            "main",
            index.clone(),
            |e| verify_package(e, &bytes),
        )
        .unwrap();
        assert_eq!(
            crate::Runtime::new(d.path(), "test")
                .unwrap()
                .apps()
                .unwrap(),
            json!([])
        );
        let path = d.path().join(".rhyven/github-registry.json");
        let before = std::fs::read(&path).unwrap();
        assert!(cache_index(
            d.path(),
            "example-publisher/registry",
            "main",
            index,
            |_| Err(Error::new("network", "test outage"))
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(cached(d.path()).unwrap().len(), 1);
        let mut cache: Value = serde_json::from_slice(&before).unwrap();
        cache["packages"][0]["manifest"]["description"] = json!("tampered");
        store::write(&path, &cache).unwrap();
        assert!(cached(d.path()).is_err());
    }
}
