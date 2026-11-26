//! Building requests and asking the server about a file.

use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder, Response, StatusCode};

use crate::error::{Error, Result};
use crate::filename;
use crate::model::{ProbeInfo, RequestInfo};

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn build_client() -> Result<Client> {
    Ok(Client::builder()
        // Ranges must be byte offsets of the real file, so ask for no compression.
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(20))
        .build()?)
}

/// A GET for `url` carrying the browser's cookies, referrer and user agent.
pub fn get(client: &Client, req: &RequestInfo, url: &str) -> Result<RequestBuilder> {
    let parsed = reqwest::Url::parse(url).map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::InvalidUrl(format!(
            "{url}: only http and https are supported"
        )));
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
    let ua = req.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
    if let Ok(v) = HeaderValue::from_str(ua) {
        headers.insert(header::USER_AGENT, v);
    }
    if let Some(r) = req
        .referer
        .as_deref()
        .and_then(|r| HeaderValue::from_str(r).ok())
    {
        headers.insert(header::REFERER, r);
    }
    if let Some(c) = req
        .cookies
        .as_deref()
        .filter(|c| !c.is_empty())
        .and_then(|c| HeaderValue::from_str(c).ok())
    {
        headers.insert(header::COOKIE, c);
    }
    for (k, v) in &req.headers {
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) else {
            continue;
        };
        // Never let a captured header break range handling.
        if name == header::RANGE || name == header::ACCEPT_ENCODING || name == header::HOST {
            continue;
        }
        headers.insert(name, value);
    }
    Ok(client.get(parsed).headers(headers))
}

/// Maps refusal codes to `LinkRefused` and other failures to `HttpStatus`.
pub fn check_status(resp: &Response) -> Result<()> {
    let s = resp.status();
    if s.is_success() {
        return Ok(());
    }
    Err(match s {
        StatusCode::UNAUTHORIZED
        | StatusCode::FORBIDDEN
        | StatusCode::NOT_FOUND
        | StatusCode::GONE => Error::LinkRefused(s.as_u16()),
        _ => Error::HttpStatus(s.as_u16()),
    })
}

/// Parses `Content-Range: bytes 0-0/12345`. Returns (start, total).
pub fn parse_content_range(resp: &Response) -> Option<(u64, Option<u64>)> {
    let v = resp.headers().get(header::CONTENT_RANGE)?.to_str().ok()?;
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    let total = total.trim().parse().ok();
    Some((start.trim().parse().ok()?, total))
}

fn header_str(resp: &Response, name: HeaderName) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Asks for the first byte to learn the size, the name and whether ranges work.
pub async fn probe(client: &Client, req: &RequestInfo) -> Result<ProbeInfo> {
    let resp = get(client, req, &req.url)?
        .header(header::RANGE, "bytes=0-0")
        .send()
        .await?;
    check_status(&resp)?;
    let final_url = resp.url().to_string();
    let content_type = header_str(&resp, header::CONTENT_TYPE);
    let disposition = header_str(&resp, header::CONTENT_DISPOSITION);
    let etag = header_str(&resp, header::ETAG);
    let last_modified = header_str(&resp, header::LAST_MODIFIED);

    let (size, resumable) = if resp.status() == StatusCode::PARTIAL_CONTENT {
        match parse_content_range(&resp) {
            Some((0, total)) => (total, total.is_some()),
            _ => (None, false),
        }
    } else {
        (resp.content_length(), false)
    };
    // Don't read the body: dropping the response closes the connection.
    drop(resp);

    let file_name = filename::choose(
        disposition.as_deref(),
        &final_url,
        &req.url,
        content_type.as_deref(),
    );
    Ok(ProbeInfo {
        final_url,
        size,
        resumable,
        file_name,
        content_type,
        etag,
        last_modified,
    })
}
