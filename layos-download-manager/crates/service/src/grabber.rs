//! Site grabber: finds the files a website links to.
//!
//! Starting from one page it follows links to other pages, up to `depth` clicks away,
//! and collects links to files of the wanted types. It stays on the starting site
//! unless told otherwise, honours the site's robots.txt, reads at most `max_pages`
//! pages, and never downloads anything itself: the user picks from the list.

use std::collections::{HashSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabOptions {
    pub start_url: String,
    /// 0 = only the starting page; 1 = also pages it links to; and so on (at most 3).
    pub depth: u8,
    /// Extensions to collect, e.g. ["pdf", "zip"].
    pub file_types: Vec<String>,
    /// Only follow and collect links on the starting site.
    #[serde(default = "yes")]
    pub same_site: bool,
    #[serde(default = "default_max_pages")]
    pub max_pages: usize,
}

fn yes() -> bool {
    true
}
fn default_max_pages() -> usize {
    150
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabbedFile {
    pub url: String,
    pub file_name: String,
    /// The page that links to it (sent as the referrer when downloading).
    pub page_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabResult {
    pub files: Vec<GrabbedFile>,
    pub pages_read: usize,
    /// Pages left out because robots.txt asks crawlers not to read them.
    pub pages_skipped_by_robots: usize,
    /// True when `max_pages` was reached before everything was read.
    pub stopped_early: bool,
}

static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:href|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>"']+))"#).unwrap()
});

/// Every link (href / src) in a page, made absolute and without #fragments.
pub fn links(html: &str, base: &Url) -> Vec<Url> {
    LINK.captures_iter(html)
        .filter_map(|c| c.get(1).or(c.get(2)).or(c.get(3)))
        .map(|m| m.as_str().trim().replace("&amp;", "&"))
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.to_ascii_lowercase().starts_with("javascript:")
        })
        .filter_map(|l| base.join(&l).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|mut u| {
            u.set_fragment(None);
            u
        })
        .collect()
}

fn extension(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    match last.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Paths a site asks all crawlers (`User-agent: *`) not to read.
pub fn robots_disallowed(robots: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut applies = false;
    let mut in_group_header = false;
    for line in robots.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match key.as_str() {
            "user-agent" => {
                // Consecutive user-agent lines form one group.
                if !in_group_header {
                    applies = false;
                }
                in_group_header = true;
                applies |= value == "*";
            }
            "disallow" => {
                in_group_header = false;
                if applies && !value.is_empty() {
                    out.push(value.to_string());
                }
            }
            _ => in_group_header = false,
        }
    }
    out
}

fn same_site(a: &Url, b: &Url) -> bool {
    a.host_str().map(|h| h.trim_start_matches("www."))
        == b.host_str().map(|h| h.trim_start_matches("www."))
}

pub async fn grab(client: &reqwest::Client, opts: &GrabOptions) -> Result<GrabResult, String> {
    let start =
        Url::parse(opts.start_url.trim()).map_err(|e| format!("{}: {e}", opts.start_url))?;
    if !matches!(start.scheme(), "http" | "https") {
        return Err("the address must start with http:// or https://".into());
    }
    let types: HashSet<String> = opts
        .file_types
        .iter()
        .map(|t| t.trim().trim_start_matches('.').to_ascii_lowercase())
        .collect();
    if types.is_empty() {
        return Err("choose at least one file type".into());
    }
    let depth = opts.depth.min(3);
    let max_pages = opts.max_pages.clamp(1, 1000);

    let disallowed = match client
        .get(start.join("/robots.txt").map_err(|e| e.to_string())?)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => robots_disallowed(&r.text().await.unwrap_or_default()),
        _ => Vec::new(),
    };
    let blocked = |u: &Url| {
        same_site(u, &start) && disallowed.iter().any(|d| u.path().starts_with(d.as_str()))
    };

    let mut result = GrabResult::default();
    let mut seen_pages: HashSet<Url> = HashSet::new();
    let mut seen_files: HashSet<Url> = HashSet::new();
    let mut queue = VecDeque::from([(start.clone(), 0u8)]);
    seen_pages.insert(start.clone());

    while let Some((page, level)) = queue.pop_front() {
        if result.pages_read >= max_pages {
            result.stopped_early = true;
            break;
        }
        if blocked(&page) {
            result.pages_skipped_by_robots += 1;
            continue;
        }
        let Ok(resp) = client.get(page.clone()).send().await else {
            continue;
        };
        let is_html = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.contains("html"));
        if !resp.status().is_success() || !is_html {
            continue;
        }
        let base = resp.url().clone();
        let Ok(html) = resp.text().await else {
            continue;
        };
        result.pages_read += 1;
        for link in links(&html, &base) {
            if opts.same_site && !same_site(&link, &start) {
                continue;
            }
            let ext = extension(&link);
            if types.contains(&ext) {
                if seen_files.insert(link.clone()) {
                    let name = link
                        .path_segments()
                        .and_then(|mut s| s.next_back())
                        .map(|n| {
                            percent_encoding::percent_decode_str(n)
                                .decode_utf8_lossy()
                                .into_owned()
                        })
                        .unwrap_or_default();
                    result.files.push(GrabbedFile {
                        url: link.to_string(),
                        file_name: name,
                        page_url: page.to_string(),
                    });
                }
            } else if level < depth
                && same_site(&link, &start)
                && matches!(
                    ext.as_str(),
                    "" | "html" | "htm" | "php" | "asp" | "aspx" | "jsp"
                )
                && seen_pages.insert(link.clone())
            {
                queue.push_back((link, level + 1));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_in_any_quoting() {
        let base = Url::parse("https://x.com/docs/").unwrap();
        let html = r##"<a href="a.pdf">A</a> <a href='/b.zip#x'>B</a> <img src=c.png> <a href="#top">t</a>
            <a href="javascript:void(0)">j</a> <a href="mailto:me@x.com">m</a> <a href="?p=2&amp;q=1">next</a>"##;
        let got: Vec<String> = links(html, &base).iter().map(|u| u.to_string()).collect();
        assert_eq!(
            got,
            [
                "https://x.com/docs/a.pdf",
                "https://x.com/b.zip",
                "https://x.com/docs/c.png",
                "https://x.com/docs/?p=2&q=1"
            ]
        );
    }

    #[test]
    fn reads_robots_txt_for_everyone() {
        let robots = "User-agent: Googlebot\nDisallow: /only-google\n\nUser-agent: *\nUser-agent: other\nDisallow: /private # no\nDisallow:\nAllow: /x\n";
        assert_eq!(robots_disallowed(robots), ["/private"]);
    }
}
