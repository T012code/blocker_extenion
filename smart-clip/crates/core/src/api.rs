//! The clips table (through PostgREST) and the image files (through Storage).

use reqwest::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde_json::json;

use crate::error::Error;
use crate::model::{Clip, Config, NewClip};

/// How many clips the app keeps per account (the server keeps the same number).
pub const MAX_CLIPS: usize = 200;

#[derive(Clone)]
pub struct Api {
    http: Client,
    cfg: Config,
}

/// Just enough of a clip to tell what changed while a PC was offline.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ClipStamp {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, Default)]
struct RestError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default, rename = "statusCode")]
    status_code: Option<String>,
}

async fn rest_error(resp: Response) -> Error {
    let status = resp.status().as_u16();
    let body: RestError = resp.json().await.unwrap_or_default();
    match (status, body.code.as_deref()) {
        // PGRST301/303: the access token ran out or isn't valid.
        (401, _) | (_, Some("PGRST301" | "PGRST303")) => Error::SignedOut,
        (429, _) => Error::TooMany,
        _ => Error::Server { status, message: body.message.or(body.error).or(body.status_code).unwrap_or_default() },
    }
}

async fn ok(resp: Response) -> Result<Response, Error> {
    if resp.status().is_success() { Ok(resp) } else { Err(rest_error(resp).await) }
}

impl Api {
    pub fn new(http: Client, cfg: Config) -> Api {
        Api { http, cfg }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    fn rest(&self, method: reqwest::Method, token: &str, query: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}/rest/v1/clips{query}", self.cfg.base()))
            .header("apikey", self.cfg.publishable_key.trim())
            .bearer_auth(token)
    }

    /// The ids and times of all the account's clips, newest first. Cheap: no content.
    pub async fn stamps(&self, token: &str) -> Result<Vec<ClipStamp>, Error> {
        let resp = self
            .rest(
                reqwest::Method::GET,
                token,
                &format!("?select=id,created_at&order=created_at.desc&limit={MAX_CLIPS}"),
            )
            .send()
            .await?;
        Ok(ok(resp).await?.json().await?)
    }

    /// Full clips, by id.
    pub async fn clips_by_id(&self, token: &str, ids: &[String]) -> Result<Vec<Clip>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // Ids are UUIDs from the server; anything else is dropped rather than sent.
        let list: Vec<&str> = ids.iter().map(String::as_str).filter(|id| is_uuid(id)).collect();
        let resp =
            self.rest(reqwest::Method::GET, token, &format!("?select=*&id=in.({})", list.join(","))).send().await?;
        Ok(ok(resp).await?.json().await?)
    }

    /// Saves a clip. Copying the same content again (same hash) bumps the old clip to the
    /// top and restarts its clock instead of adding a second one.
    pub async fn upsert(&self, token: &str, clip: &NewClip) -> Result<Clip, Error> {
        let resp = self
            .rest(reqwest::Method::POST, token, "?on_conflict=user_id,hash")
            .header("Prefer", "resolution=merge-duplicates,return=representation")
            .json(clip)
            .send()
            .await?;
        let mut rows: Vec<Clip> = ok(resp).await?.json().await?;
        rows.pop().ok_or(Error::Server { status: 0, message: "the clip wasn't saved".into() })
    }

    pub async fn delete(&self, token: &str, id: &str) -> Result<(), Error> {
        if !is_uuid(id) {
            return Ok(());
        }
        let resp = self.rest(reqwest::Method::DELETE, token, &format!("?id=eq.{id}")).send().await?;
        ok(resp).await.map(|_| ())
    }

    /// Deletes every clip of the account (row-level security limits it to the account).
    pub async fn delete_all(&self, token: &str) -> Result<(), Error> {
        let resp = self.rest(reqwest::Method::DELETE, token, "?id=not.is.null").send().await?;
        ok(resp).await.map(|_| ())
    }

    fn storage(&self, method: reqwest::Method, token: &str, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}/storage/v1/object/{path}", self.cfg.base()))
            .header("apikey", self.cfg.publishable_key.trim())
            .bearer_auth(token)
    }

    pub async fn upload_image(&self, token: &str, path: &str, bytes: Vec<u8>, mime: &str) -> Result<(), Error> {
        let resp = self
            .storage(reqwest::Method::POST, token, &format!("clips/{path}"))
            .header("Content-Type", mime)
            .header("x-upsert", "false")
            .header("Cache-Control", "max-age=86400")
            .body(bytes)
            .send()
            .await?;
        ok(resp).await.map(|_| ())
    }

    pub async fn download_image(&self, token: &str, path: &str) -> Result<Vec<u8>, Error> {
        let resp = self.storage(reqwest::Method::GET, token, &format!("authenticated/clips/{path}")).send().await?;
        Ok(ok(resp).await?.bytes().await?.to_vec())
    }

    /// Removes image files. The hourly clean-up catches any this misses.
    pub async fn remove_images(&self, token: &str, paths: &[String]) -> Result<(), Error> {
        if paths.is_empty() {
            return Ok(());
        }
        let resp =
            self.storage(reqwest::Method::DELETE, token, "clips").json(&json!({ "prefixes": paths })).send().await?;
        ok(resp).await.map(|_| ())
    }
}

pub fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids() {
        assert!(is_uuid("0b0e8c0e-6f0e-4a5e-9a7b-1c2d3e4f5a6b"));
        assert!(!is_uuid("0b0e8c0e-6f0e-4a5e-9a7b-1c2d3e4f5a6"));
        assert!(!is_uuid("0b0e8c0e-6f0e-4a5e-9a7b,1c2d3e4f5a6b"));
        assert!(!is_uuid("x); drop table clips; --xxxxxxxxxxxxxxxx"));
    }
}
