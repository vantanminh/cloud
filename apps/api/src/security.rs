use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{config::Config, error::AppError};

pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    use argon2::{Argon2, PasswordHasher, password_hash::SaltString};

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(AppError::internal)
}

pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let Ok(parsed_hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn get_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                (key == name).then(|| value.to_owned())
            })
        })
}

pub fn cookie_header(
    name: &str,
    value: &str,
    max_age_seconds: i64,
    http_only: bool,
    secure: bool,
) -> HeaderValue {
    let mut cookie = format!("{name}={value}; Max-Age={max_age_seconds}; Path=/; SameSite=Lax");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("cookie values are header-safe")
}

pub fn append_cookie(response: &mut Response, cookie: HeaderValue) {
    response.headers_mut().append(header::SET_COOKIE, cookie);
}

pub fn require_csrf(headers: &HeaderMap, config: &Config) -> Result<(), AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden {
            code: "ORIGIN_REQUIRED",
            message: "The request origin is not allowed.",
        })?;
    if !config.is_allowed_origin(origin) {
        return Err(AppError::Forbidden {
            code: "ORIGIN_NOT_ALLOWED",
            message: "The request origin is not allowed.",
        });
    }

    let cookie_token =
        get_cookie(headers, config.csrf_cookie_name()).ok_or(AppError::Forbidden {
            code: "CSRF_REQUIRED",
            message: "A CSRF token is required.",
        })?;
    let submitted_token = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden {
            code: "CSRF_REQUIRED",
            message: "A CSRF token is required.",
        })?;

    if cookie_token
        .as_bytes()
        .ct_eq(submitted_token.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(AppError::Forbidden {
            code: "CSRF_INVALID",
            message: "The CSRF token is invalid.",
        });
    }

    Ok(())
}
