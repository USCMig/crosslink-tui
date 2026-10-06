use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Rpc {
    pub url: String,
}

impl Rpc {
    pub fn new(url: &str) -> Self {
        Self { url: url.to_string() }
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call_with_timeout(method, params, Duration::from_secs(4))
    }

    pub fn call_with_timeout(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let reply: Value = match ureq::post(&self.url).timeout(timeout).send_json(body) {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
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
