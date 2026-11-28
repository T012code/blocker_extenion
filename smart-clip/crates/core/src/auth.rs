//! Signing in with a code sent by email (Supabase Auth's email OTP), like Blip: type
//! your email, then the code. New emails get an account automatically.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::error::Error;
use crate::model::{Config, StoredSession};

/// A signed-in session.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// When the access token runs out (Unix seconds).
    pub expires_at: u64,
    pub user_id: String,
    pub email: String,
}

impl Session {
    /// A session restored from disk: it has to be refreshed before use.
    pub fn restored(s: &StoredSession) -> Session {
        Session {
            access_token: String::new(),
            refresh_token: s.refresh_token.clone(),
            expires_at: 0,
            user_id: s.user_id.clone(),
            email: s.email.clone(),
        }
    }

    pub fn stored(&self) -> StoredSession {
        StoredSession {
            refresh_token: self.refresh_token.clone(),
            user_id: self.user_id.clone(),
            email: self.email.clone(),
        }
    }

    /// Seconds until the access token should be refreshed (a minute before it runs out).
    pub fn refresh_in(&self) -> u64 {
        self.expires_at.saturating_sub(now() + 60)
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A light check: something@something.something, no spaces.
pub fn valid_email(email: &str) -> bool {
    let e = email.trim();
    let Some((user, domain)) = e.split_once('@') else { return false };
    !user.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !e.chars().any(|c| c.is_whitespace())
        && !domain.contains('@')
        && e.len() <= 254
}

/// Keeps only the digits of a typed code ("123 456" and "123-456" both work).
pub fn clean_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_digit()).collect()
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    expires_at: Option<u64>,
    user: UserReply,
}

#[derive(Deserialize)]
struct UserReply {
    id: String,
    #[serde(default)]
    email: Option<String>,
}

/// Supabase Auth error bodies come in a few shapes; this reads them all.
#[derive(Deserialize, Default)]
struct AuthErrorReply {
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn auth_error(resp: Response) -> Error {
    let status = resp.status();
    let body: AuthErrorReply = resp.json().await.unwrap_or_default();
    let code = body
        .error_code
        .or_else(|| body.code.and_then(|c| c.as_str().map(str::to_owned)))
        .or(body.error)
        .unwrap_or_default();
    let message = body.msg.or(body.message).or(body.error_description).unwrap_or_default();
    classify(status, &code, &message)
}

fn classify(status: StatusCode, code: &str, message: &str) -> Error {
    match code {
        "otp_expired" | "otp_disabled" => return Error::BadCode,
        "over_email_send_rate_limit" | "over_request_rate_limit" => return Error::TooMany,
        "email_address_invalid" | "validation_failed" => {
            return Error::Invalid("That email address doesn't look right.".into());
        }
        "email_address_not_authorized" => {
            return Error::Invalid(
                "The server can't email that address yet. Its owner needs to set up email sending (SMTP).".into(),
            );
        }
        "refresh_token_not_found"
        | "refresh_token_already_used"
        | "session_not_found"
        | "session_expired"
        | "user_not_found"
        | "user_banned"
        | "bad_jwt" => return Error::SignedOut,
        "signup_disabled" => return Error::Invalid("New accounts are turned off on this server.".into()),
        _ => {}
    }
    match status.as_u16() {
        429 => Error::TooMany,
        401 | 403 => Error::SignedOut,
        400 if code == "invalid_grant" => Error::SignedOut,
        s => Error::Server { status: s, message: message.to_owned() },
    }
}

fn session_from(reply: TokenReply, fallback_email: &str) -> Session {
    let expires_at = reply.expires_at.unwrap_or_else(|| now() + reply.expires_in.unwrap_or(3600));
    Session {
        access_token: reply.access_token,
        refresh_token: reply.refresh_token,
        expires_at,
        user_id: reply.user.id,
        email: reply.user.email.filter(|e| !e.is_empty()).unwrap_or_else(|| fallback_email.to_owned()),
    }
}

/// Emails a sign-in code. Creates the account if the email is new.
pub async fn send_code(http: &Client, cfg: &Config, email: &str) -> Result<(), Error> {
    let email = email.trim().to_lowercase();
    if !valid_email(&email) {
        return Err(Error::Invalid("That email address doesn't look right.".into()));
    }
    let resp = http
        .post(format!("{}/auth/v1/otp", cfg.base()))
        .header("apikey", cfg.publishable_key.trim())
        .json(&json!({ "email": email, "create_user": true }))
        .send()
        .await?;
    if resp.status().is_success() { Ok(()) } else { Err(auth_error(resp).await) }
}

/// Checks the emailed code and signs in.
pub async fn verify_code(http: &Client, cfg: &Config, email: &str, code: &str) -> Result<Session, Error> {
    let email = email.trim().to_lowercase();
    let code = clean_code(code);
    if !(6..=10).contains(&code.len()) {
        return Err(Error::Invalid("Type the code from the email.".into()));
    }
    let resp = http
        .post(format!("{}/auth/v1/verify", cfg.base()))
        .header("apikey", cfg.publishable_key.trim())
        .json(&json!({ "type": "email", "email": email, "token": code }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(match auth_error(resp).await {
            // A wrong code comes back as a plain 400/403.
            Error::SignedOut | Error::Server { status: 400, .. } => Error::BadCode,
            e => e,
        });
    }
    Ok(session_from(resp.json().await?, &email))
}

/// Gets a fresh access token (and a new refresh token: they're single-use).
pub async fn refresh(http: &Client, cfg: &Config, s: &Session) -> Result<Session, Error> {
    let resp = http
        .post(format!("{}/auth/v1/token?grant_type=refresh_token", cfg.base()))
        .header("apikey", cfg.publishable_key.trim())
        .json(&json!({ "refresh_token": s.refresh_token }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(match auth_error(resp).await {
            Error::Server { status: 400, .. } => Error::SignedOut,
            e => e,
        });
    }
    Ok(session_from(resp.json().await?, &s.email))
}

/// Ends this PC's session on the server. Best effort: signing out locally always works.
pub async fn sign_out(http: &Client, cfg: &Config, s: &Session) {
    if s.access_token.is_empty() {
        return;
    }
    let _ = http
        .post(format!("{}/auth/v1/logout?scope=local", cfg.base()))
        .header("apikey", cfg.publishable_key.trim())
        .bearer_auth(&s.access_token)
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert!(valid_email("layo@example.com"));
        assert!(valid_email("  a.b+c@mail.co.uk "));
        assert!(!valid_email("layo"));
        assert!(!valid_email("layo@example"));
        assert!(!valid_email("@example.com"));
        assert!(!valid_email("la yo@example.com"));
        assert!(!valid_email("a@b@c.com"));
        assert!(!valid_email("a@.com"));
    }

    #[test]
    fn codes() {
        assert_eq!(clean_code(" 123 456 "), "123456");
        assert_eq!(clean_code("123-456"), "123456");
    }

    #[test]
    fn errors() {
        assert_eq!(classify(StatusCode::FORBIDDEN, "otp_expired", ""), Error::BadCode);
        assert_eq!(classify(StatusCode::TOO_MANY_REQUESTS, "", ""), Error::TooMany);
        assert_eq!(classify(StatusCode::BAD_REQUEST, "refresh_token_not_found", ""), Error::SignedOut);
        assert!(matches!(classify(StatusCode::BAD_GATEWAY, "", "x"), Error::Server { status: 502, .. }));
    }

    #[test]
    fn refresh_timing() {
        let s = Session {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: now() + 3600,
            user_id: "u".into(),
            email: "e".into(),
        };
        assert!((3530..=3540).contains(&s.refresh_in()));
        assert_eq!(Session { expires_at: 0, ..s }.refresh_in(), 0);
    }
}
