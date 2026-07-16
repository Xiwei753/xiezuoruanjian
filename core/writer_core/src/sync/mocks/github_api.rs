use base64::Engine;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::sync::types::SyncManifest;

#[allow(clippy::type_complexity)]
pub fn start_mock_github_api(
    initial_manifest: Option<SyncManifest>,
    initial_files: std::collections::HashMap<String, String>,
) -> (
    String,
    Arc<AtomicBool>,
    Arc<Mutex<std::collections::HashMap<String, String>>>,
    Arc<Mutex<String>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = format!("http://127.0.0.1:{}", port);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let files = Arc::new(Mutex::new(initial_files));
    let files_clone = files.clone();

    let manifest_str = if let Some(m) = initial_manifest {
        serde_json::to_string(&m).unwrap()
    } else {
        String::new()
    };
    let manifest = Arc::new(Mutex::new(manifest_str));
    let manifest_clone = manifest.clone();

    listener.set_nonblocking(true).unwrap();

    let handle = thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0; 65536];
                    if let Ok(bytes_read) = stream.read(&mut buffer) {
                        let req = String::from_utf8_lossy(&buffer[..bytes_read]);
                        let first_line = req.lines().next().unwrap_or("");
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let method = parts[0];
                            let path = parts[1];

                            let mut response_body = String::new();
                            let mut status_line = "HTTP/1.1 200 OK";

                            if path.contains("/rate_limit") {
                                response_body = r#"{"resources":{}}"#.to_string();
                            } else if path.contains("/git/ref/heads/main") {
                                let m = manifest_clone.lock().unwrap();
                                if m.is_empty() {
                                    status_line = "HTTP/1.1 404 Not Found";
                                    response_body = r#"{"message":"Not Found"}"#.to_string();
                                } else {
                                    response_body =
                                        r#"{"object":{"sha":"mock_commit_sha"}}"#.to_string();
                                }
                            } else if path.contains("/git/commits/mock_commit_sha") {
                                response_body = r#"{"tree":{"sha":"mock_tree_sha"}}"#.to_string();
                            } else if path.contains("/git/trees/mock_tree_sha")
                                || path.contains("/git/trees/main")
                            {
                                let mut tree_list = Vec::new();
                                let m = manifest_clone.lock().unwrap();
                                if !m.is_empty() {
                                    tree_list.push(serde_json::json!({
                                        "path": "app-meta/sync/manifest.sync.json",
                                        "type": "blob",
                                        "sha": "manifest_blob_sha"
                                    }));
                                }
                                let fls = files_clone.lock().unwrap();
                                for filename in fls.keys() {
                                    tree_list.push(serde_json::json!({
                                        "path": filename,
                                        "type": "blob",
                                        "sha": format!("{}_sha", filename)
                                    }));
                                }
                                response_body =
                                    serde_json::json!({ "tree": tree_list }).to_string();
                            } else if path.contains("/contents/app-meta/sync/manifest.sync.json") {
                                let m = manifest_clone.lock().unwrap();
                                if method == "GET" {
                                    if m.is_empty() {
                                        status_line = "HTTP/1.1 404 Not Found";
                                    } else {
                                        let encoded = base64::engine::general_purpose::STANDARD
                                            .encode(m.as_bytes());
                                        response_body = serde_json::json!({
                                            "content": encoded,
                                            "encoding": "base64",
                                            "sha": "manifest_blob_sha"
                                        })
                                        .to_string();
                                    }
                                } else if method == "PUT" {
                                    let manifest_exists = !m.is_empty();
                                    drop(m);
                                    if let Some(body_start) = req.find("\r\n\r\n") {
                                        let body = &req[body_start + 4..];
                                        if let Ok(val) =
                                            serde_json::from_str::<serde_json::Value>(body)
                                        {
                                            if manifest_exists && val["sha"].as_str().is_none() {
                                                status_line = "HTTP/1.1 422 Unprocessable Entity";
                                                response_body =
                                                    r#"{"message":"sha required"}"#.to_string();
                                            }
                                            if let Some(b64_content) = val["content"].as_str() {
                                                if status_line == "HTTP/1.1 200 OK" {
                                                    let decoded =
                                                        base64::engine::general_purpose::STANDARD
                                                            .decode(b64_content)
                                                            .unwrap();
                                                    let mut m = manifest_clone.lock().unwrap();
                                                    *m = String::from_utf8(decoded).unwrap();
                                                }
                                            }
                                        }
                                    }
                                    if status_line == "HTTP/1.1 200 OK" {
                                        response_body =
                                            r#"{"content":{"sha":"manifest_new_sha"}}"#.to_string();
                                    }
                                } else {
                                    status_line = "HTTP/1.1 405 Method Not Allowed";
                                }
                            } else if path.contains("/contents/") {
                                if let Some(idx) = path.find("/contents/") {
                                    let file_path = &path[idx + 10..];
                                    let file_path =
                                        file_path.split('?').next().unwrap_or(file_path);

                                    if method == "GET" {
                                        let fls = files_clone.lock().unwrap();
                                        if let Some(content) = fls.get(file_path) {
                                            let encoded = base64::engine::general_purpose::STANDARD
                                                .encode(content.as_bytes());
                                            response_body = serde_json::json!({
                                                "content": encoded,
                                                "encoding": "base64",
                                                "sha": format!("{}_sha", file_path)
                                            })
                                            .to_string();
                                        } else {
                                            status_line = "HTTP/1.1 404 Not Found";
                                        }
                                    } else if method == "PUT" {
                                        if let Some(body_start) = req.find("\r\n\r\n") {
                                            let body = &req[body_start + 4..];
                                            if let Ok(val) =
                                                serde_json::from_str::<serde_json::Value>(body)
                                            {
                                                let file_exists = files_clone
                                                    .lock()
                                                    .unwrap()
                                                    .contains_key(file_path);
                                                if file_exists && val["sha"].as_str().is_none() {
                                                    status_line =
                                                        "HTTP/1.1 422 Unprocessable Entity";
                                                    response_body =
                                                        r#"{"message":"sha required"}"#.to_string();
                                                }
                                                if let Some(b64_content) = val["content"].as_str() {
                                                    if status_line == "HTTP/1.1 200 OK" {
                                                        let decoded = base64::engine::general_purpose::STANDARD
                                                            .decode(b64_content)
                                                            .unwrap();
                                                        let mut fls = files_clone.lock().unwrap();
                                                        fls.insert(
                                                            file_path.to_string(),
                                                            String::from_utf8(decoded).unwrap(),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        if status_line == "HTTP/1.1 200 OK" {
                                            response_body =
                                                r#"{"content":{"sha":"new_sha"}}"#.to_string();
                                        }
                                    } else if method == "DELETE" {
                                        if let Some(body_start) = req.find("\r\n\r\n") {
                                            let body = &req[body_start + 4..];
                                            let val =
                                                serde_json::from_str::<serde_json::Value>(body)
                                                    .unwrap_or_default();
                                            if val["sha"].as_str().is_none() {
                                                status_line = "HTTP/1.1 422 Unprocessable Entity";
                                                response_body =
                                                    r#"{"message":"sha required"}"#.to_string();
                                            } else {
                                                let mut fls = files_clone.lock().unwrap();
                                                fls.remove(file_path);
                                                response_body = r#"{"content":null}"#.to_string();
                                            }
                                        }
                                    } else {
                                        status_line = "HTTP/1.1 405 Method Not Allowed";
                                    }
                                }
                            } else if method == "POST" && path.contains("/git/blobs") {
                                status_line = "HTTP/1.1 500 Internal Server Error";
                                if let Some(body_start) = req.find("\r\n\r\n") {
                                    let body = &req[body_start + 4..];
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(body)
                                    {
                                        if let Some(b64_content) = val["content"].as_str() {
                                            if let Ok(decoded_bytes) =
                                                base64::engine::general_purpose::STANDARD
                                                    .decode(b64_content)
                                            {
                                                if let Ok(decoded_str) =
                                                    String::from_utf8(decoded_bytes)
                                                {
                                                    if decoded_str.contains("manifest.sync.json")
                                                        || decoded_str.contains("\"files\":")
                                                    {
                                                        let mut m = manifest_clone.lock().unwrap();
                                                        *m = decoded_str;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                response_body =
                                    r#"{"message":"git db api must not be used"}"#.to_string();
                            } else if method == "POST" && path.contains("/git/trees") {
                                status_line = "HTTP/1.1 500 Internal Server Error";
                                response_body =
                                    r#"{"message":"git db api must not be used"}"#.to_string();
                            } else if method == "POST" && path.contains("/git/commits") {
                                status_line = "HTTP/1.1 500 Internal Server Error";
                                response_body =
                                    r#"{"message":"git db api must not be used__"}"#.to_string();
                            } else if method == "POST" && path.contains("/git/refs") {
                                status_line = "HTTP/1.1 500 Internal Server Error";
                                response_body =
                                    r#"{"message":"git db api must not be used"}"#.to_string();
                            } else if method == "PATCH" && path.contains("/git/refs/heads/main") {
                                status_line = "HTTP/1.1 500 Internal Server Error";
                                response_body =
                                    r#"{"message":"git db api must not be used_"}"#.to_string();
                            }

                            let response = format!(
                                "{}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                                status_line,
                                response_body.len(),
                                response_body
                            );
                            let _ = stream.write_all(response.as_bytes());
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => {}
            }
        }
    });

    (addr, shutdown, files, manifest, handle)
}
