use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
pub struct Rpc {
    pub url: String,
    /// zebrad's `.cookie` file. From v17 the node requires it unless `rpc.enable_cookie_auth` is
    /// false. zebrad writes a new cookie each time it starts, so it is read again on every call.
    pub cookie: Option<PathBuf>,
}

impl Rpc {
    pub fn new(url: &str, cookie: Option<PathBuf>) -> Self {
        Self { url: url.to_string(), cookie }
    }

    /// `Basic` credentials from the cookie file, which holds `__cookie__:<secret>`. `None` when
    /// there is no cookie, as on a node with cookie auth off.
    fn auth_header(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.cookie.as_ref()?).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| format!("Basic {}", base64(text.as_bytes())))
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call_with_timeout(method, params, Duration::from_secs(4))
    }

    pub fn call_with_timeout(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let mut req = ureq::post(&self.url).timeout(timeout);
        if let Some(auth) = self.auth_header() {
            req = req.set("Authorization", &auth);
        }
        let reply: Value = match req.send_json(body) {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(401, _)) => return Err(self.unauthorized()),
            // JSON-RPC errors come back with a non-200 status but still carry the error body.
            Err(ureq::Error::Status(_, r)) => r.into_json().map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        };
        if let Some(err) = reply.get("error").filter(|e| !e.is_null()) {
            return Err(err
                .get("message")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| err.to_string()));
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }
}

impl Rpc {
    fn unauthorized(&self) -> String {
        match &self.cookie {
            Some(p) if p.exists() => format!("the node refused the RPC cookie in {} (HTTP 401)", p.display()),
            Some(p) => format!(
                "the node wants RPC cookie auth (HTTP 401) and there is no cookie at {}: pass --rpc-cookie PATH",
                p.display()
            ),
            None => "the node wants RPC cookie auth (HTTP 401): pass --rpc-cookie PATH".into(),
        }
    }
}

/// Where zebrad writes its cookie: `[rpc] cookie_dir`, else its default cache directory
/// (`dirs::cache_dir()/zebra`). That default is not `[state] cache_dir`.
pub fn default_cookie_path(cookie_dir: Option<String>) -> PathBuf {
    let dir = cookie_dir.map(PathBuf::from).unwrap_or_else(|| {
        let home = || PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let cache = if cfg!(target_os = "windows") {
            PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
        } else if cfg!(target_os = "macos") {
            home().join("Library/Caches")
        } else {
            std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".cache"))
        };
        cache.join("zebra")
    });
    dir.join(".cookie")
}

/// Standard base64 with padding, which is all HTTP Basic auth needs.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// GET a JSON document. Used for the public bonded-stake board, which is not a node RPC.
pub fn get_json(url: &str, timeout: Duration) -> Result<Value, String> {
    match ureq::get(url)
        .set("User-Agent", "crosslink-tui/0.1")
        .set("Accept", "application/json")
        .timeout(timeout)
        .call()
    {
        Ok(r) => r.into_json().map_err(|e| format!("response was not JSON ({e})")),
        Err(ureq::Error::Status(code, r)) => {
            let _ = r.into_string();
            Err(format!("HTTP {code}"))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("__cookie__:ab", "X19jb29raWVfXzphYg==")] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn cookie_path_prefers_the_configured_dir() {
        assert_eq!(default_cookie_path(Some("/srv/z".into())), PathBuf::from("/srv/z/.cookie"));
        assert!(default_cookie_path(None).ends_with("zebra/.cookie"));
    }
}
