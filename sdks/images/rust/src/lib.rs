use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Api {
        status: u16,
        code: String,
        message: String,
        fields: Option<serde_json::Map<String, Value>>,
    },
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Transport(#[from] reqwest::Error),
}

#[derive(Clone, Debug)]
pub struct Client {
    client_id: String,
    client_secret: String,
    base_url: String,
    http: reqwest::Client,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub id: String,
    pub name: String,
    pub resource_type: String,
    pub compression_mode: String,
    pub max_width: Option<i32>,
    pub max_height: Option<i32>,
    pub quality: Option<i32>,
    pub object_count: i64,
    pub byte_size: i64,
    pub public_base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub id: String,
    pub folder: String,
    pub file_name: String,
    pub content_type: String,
    pub byte_size: i64,
    pub width: i32,
    pub height: i32,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ObjectList {
    pub objects: Vec<Object>,
    pub folders: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedUrl {
    pub url: String,
    pub visibility: String,
    pub expires_at: Option<String>,
    pub cache_seconds: u32,
    pub content_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DeletedFolder {
    pub deleted: u64,
}

#[derive(Clone, Debug)]
pub struct Upload<'a> {
    pub bytes: &'a [u8],
    pub content_type: &'a str,
    pub folder: &'a str,
    pub file_name: &'a str,
}

#[derive(Clone, Debug, Default)]
pub struct ListOptions<'a> {
    pub folder: Option<&'a str>,
    pub recursive: bool,
}

#[derive(Clone, Debug)]
pub struct SignOptions {
    pub visibility: String,
    pub expires_in_seconds: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub quality: Option<u32>,
}

impl SignOptions {
    pub fn public_url() -> Self {
        Self {
            visibility: "public".to_owned(),
            expires_in_seconds: None,
            width: None,
            height: None,
            quality: None,
        }
    }
}

impl Client {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, Error> {
        let client_id = client_id.into();
        let client_secret = client_secret.into();
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(Error::Config(
                "client id and client secret are required".to_owned(),
            ));
        }
        let base_url = normalize_base_url(&base_url.into())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client_id,
            client_secret,
            base_url,
            http,
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret(&self) -> &str {
        &self.client_secret
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn get_store(&self) -> Result<Store, Error> {
        self.send(self.http.get(self.url("/images/store"))).await
    }

    pub async fn upload(&self, image: Upload<'_>) -> Result<Object, Error> {
        if image.file_name.is_empty() {
            return Err(Error::Config("file name is required".to_owned()));
        }
        self.send(
            self.http
                .post(self.url("/images/objects"))
                .header("Content-Type", image.content_type)
                .header("X-Knotree-Folder", image.folder)
                .header("X-Knotree-File-Name", image.file_name)
                .body(image.bytes.to_vec()),
        )
        .await
    }

    pub async fn list(&self, options: ListOptions<'_>) -> Result<ObjectList, Error> {
        let mut request = self.http.get(self.url("/images/objects"));
        if let Some(folder) = options.folder.filter(|folder| !folder.is_empty()) {
            request = request.query(&[("folder", folder)]);
        }
        request = request.query(&[(
            "recursive",
            if options.recursive { "true" } else { "false" },
        )]);
        self.send(request).await
    }

    pub async fn delete(&self, image_id: &str) -> Result<(), Error> {
        self.send_empty(
            self.http
                .delete(self.url(&format!("/images/objects/{}", urlencoding_path(image_id)))),
        )
        .await
    }

    pub async fn delete_folder(&self, folder: &str) -> Result<DeletedFolder, Error> {
        self.send(
            self.http
                .delete(self.url("/images/folders"))
                .query(&[("folder", folder)]),
        )
        .await
    }

    pub async fn sign_url(&self, image_id: &str, options: SignOptions) -> Result<SignedUrl, Error> {
        let mut body = serde_json::Map::new();
        body.insert("visibility".to_owned(), Value::String(options.visibility));
        if let Some(seconds) = options.expires_in_seconds {
            body.insert("expiresInSeconds".to_owned(), Value::from(seconds));
        }
        if let Some(width) = options.width {
            body.insert("width".to_owned(), Value::from(width));
        }
        if let Some(height) = options.height {
            body.insert("height".to_owned(), Value::from(height));
        }
        if let Some(quality) = options.quality {
            body.insert("quality".to_owned(), Value::from(quality));
        }
        self.send(
            self.http
                .post(self.url(&format!(
                    "/images/objects/{}/sign",
                    urlencoding_path(image_id)
                )))
                .json(&Value::Object(body)),
        )
        .await
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base_url)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("X-Knotree-Client-Id", &self.client_id)
            .header("X-Knotree-Client-Secret", &self.client_secret)
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let response = self.authorize(request).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(api_error(status.as_u16(), response).await);
        }
        Ok(response.json().await?)
    }

    async fn send_empty(&self, request: reqwest::RequestBuilder) -> Result<(), Error> {
        let response = self.authorize(request).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(api_error(status.as_u16(), response).await);
        }
        Ok(())
    }
}

fn normalize_base_url(base_url: &str) -> Result<String, Error> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Error::Config("base URL is required".to_owned()));
    }
    let trimmed = trimmed.strip_suffix("/api/v1").unwrap_or(trimmed);
    Ok(trimmed.trim_end_matches('/').to_owned())
}

fn urlencoding_path(value: &str) -> String {
    urlencoding_encode(value)
}

fn urlencoding_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

async fn api_error(status: u16, response: reqwest::Response) -> Error {
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    let error = payload.get("error");
    Error::Api {
        status,
        code: error
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("IMAGE_REQUEST_FAILED")
            .to_owned(),
        message: error
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Image request failed.")
            .to_owned(),
        fields: error
            .and_then(|value| value.get("fields"))
            .and_then(Value::as_object)
            .cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Clone, Debug)]
    struct Seen {
        request: String,
        body: String,
    }

    fn serve() -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        thread::spawn(move || {
            for stream in listener.incoming().take(6) {
                let mut stream = stream.unwrap();
                let mut buffer = Vec::new();
                let mut chunk = [0_u8; 2048];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    buffer.extend_from_slice(&chunk[..count]);
                    if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&buffer[..end]).to_string();
                        let length = content_length(&headers);
                        while buffer.len() < end + 4 + length {
                            let count = stream.read(&mut chunk).unwrap();
                            buffer.extend_from_slice(&chunk[..count]);
                        }
                        let body =
                            String::from_utf8_lossy(&buffer[end + 4..end + 4 + length]).to_string();
                        recorded.lock().unwrap().push(Seen {
                            request: headers.clone(),
                            body,
                        });
                        let response = response_for(&headers);
                        stream.write_all(response.as_bytes()).unwrap();
                        break;
                    }
                }
            }
        });
        (format!("http://127.0.0.1:{port}/api/v1"), seen)
    }

    fn content_length(headers: &str) -> usize {
        headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())
                    .flatten()
            })
            .unwrap_or(0)
    }

    fn response_for(headers: &str) -> String {
        let request: String = headers.lines().next().unwrap_or("").to_owned();
        if request.starts_with("DELETE /api/v1/images/objects/") {
            return "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_owned();
        }
        let json = if request.starts_with("GET /api/v1/images/store ") {
            r#"{"id":"store","name":"Website","resourceType":"images","compressionMode":"none","maxWidth":null,"maxHeight":null,"quality":null,"objectCount":1,"byteSize":3,"publicBaseUrl":"https://img.knotree.org"}"#
        } else if request.starts_with("POST /api/v1/images/objects ") {
            r#"{"id":"img-1","folder":"posts","fileName":"cover.png","contentType":"image/png","byteSize":3,"width":1,"height":1,"createdAt":"2026-10-02T00:00:00Z"}"#
        } else if request.starts_with("GET /api/v1/images/objects?") {
            r#"{"objects":[],"folders":["posts"]}"#
        } else if request.starts_with("DELETE /api/v1/images/folders?") {
            r#"{"deleted":1}"#
        } else if request.contains("/sign ") {
            r#"{"url":"https://img.knotree.org/images/v1/store/img-1?mode=none&sig=abc","visibility":"public","expiresAt":null,"cacheSeconds":31536000,"contentType":"image/png"}"#
        } else {
            r#"{"error":{"code":"MISSING","message":"missing"}}"#
        };
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        )
    }

    #[tokio::test]
    async fn client_signs_a_stable_url_and_sends_credentials() {
        let (base_url, seen) = serve();
        let client = Client::new("kimg_server", "ksec_secret", base_url).unwrap();
        assert!(!client.base_url().contains("/api/v1"));
        let store = client.get_store().await.unwrap();
        assert_eq!(store.compression_mode, "none");
        let object = client
            .upload(Upload {
                bytes: b"png",
                content_type: "image/png",
                folder: "posts",
                file_name: "cover.png",
            })
            .await
            .unwrap();
        assert_eq!(object.id, "img-1");
        let listed = client
            .list(ListOptions {
                folder: Some("posts"),
                recursive: true,
            })
            .await
            .unwrap();
        assert_eq!(listed.folders, vec!["posts".to_owned()]);
        let mut options = SignOptions::public_url();
        options.width = Some(640);
        let signed = client.sign_url(&object.id, options).await.unwrap();
        assert_eq!(signed.cache_seconds, 31_536_000);
        assert!(signed.url.starts_with("https://img.knotree.org/"));
        client.delete(&object.id).await.unwrap();
        assert_eq!(client.delete_folder("posts").await.unwrap().deleted, 1);

        let calls = seen.lock().unwrap().clone();
        assert_eq!(calls.len(), 6);
        assert!(calls.iter().all(|call| {
            let request = call.request.to_ascii_lowercase();
            request.contains("x-knotree-client-id: kimg_server")
                && request.contains("x-knotree-client-secret: ksec_secret")
        }));
        assert!(calls.iter().any(|call| call.body.contains("\"width\":640")));
    }

    #[test]
    fn missing_credentials_are_rejected() {
        let error = Client::new("", "secret", "http://localhost:8080").unwrap_err();
        assert!(matches!(error, Error::Config(_)));
    }
}
