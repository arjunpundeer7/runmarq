//! Blocking client for the local collector API, used by the CLI, the hook
//! path and the desktop app's native side. The token never leaves this
//! process; the desktop webview never sees it.

use crate::lifecycle;
use runmarq_core::api::{Discovery, Health, IngestRequest, IngestResponse, StreamMessage};
use runmarq_core::paths::Paths;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the runmarq collector is not running")]
    NotRunning,
    #[error("collector request failed: {0}")]
    Http(String),
    #[error("collector returned status {0}: {1}")]
    Status(u16, String),
    #[error("could not decode collector response: {0}")]
    Decode(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Clone)]
pub struct Client {
    base: String,
    auth: String,
    agent: ureq::Agent,
    pub discovery: Discovery,
}

fn map_err(e: ureq::Error) -> ClientError {
    match e {
        ureq::Error::Status(code, resp) => {
            ClientError::Status(code, resp.into_string().unwrap_or_default())
        }
        ureq::Error::Transport(t) => {
            if t.kind() == ureq::ErrorKind::ConnectionFailed {
                ClientError::NotRunning
            } else {
                ClientError::Http(t.to_string())
            }
        }
    }
}

impl Client {
    pub fn from_discovery(d: Discovery, timeout: Duration) -> Client {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout.min(Duration::from_millis(500)))
            .timeout(timeout)
            .no_delay(true)
            .build();
        Client {
            base: format!("http://127.0.0.1:{}", d.port),
            auth: format!("Bearer {}", d.token),
            agent,
            discovery: d,
        }
    }

    /// Finds the running collector via its discovery file. Does not contact it.
    pub fn discover(paths: &Paths, timeout: Duration) -> Result<Client> {
        lifecycle::read_discovery(paths)
            .map(|d| Client::from_discovery(d, timeout))
            .ok_or(ClientError::NotRunning)
    }

    fn req(&self, method: &str, path: &str) -> ureq::Request {
        self.agent
            .request(method, &format!("{}{}", self.base, path))
            .set("Authorization", &self.auth)
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.req("GET", path)
            .call()
            .map_err(map_err)?
            .into_json()
            .map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub fn get_text(&self, path: &str) -> Result<String> {
        self.req("GET", path)
            .call()
            .map_err(map_err)?
            .into_string()
            .map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.req(method, path)
            .send_json(body)
            .map_err(map_err)?
            .into_json()
            .map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub fn send_no_content<B: Serialize>(&self, method: &str, path: &str, body: &B) -> Result<()> {
        self.req(method, path).send_json(body).map_err(map_err)?;
        Ok(())
    }

    pub fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.req("DELETE", path)
            .call()
            .map_err(map_err)?
            .into_json()
            .map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub fn health(&self) -> Result<Health> {
        self.get("/v1/health")
    }

    pub fn ingest(&self, req: &IngestRequest) -> Result<IngestResponse> {
        self.send("POST", "/v1/events", req)
    }

    pub fn shutdown(&self) -> Result<()> {
        self.req("POST", "/v1/shutdown")
            .set("Content-Type", "application/json")
            .send_string("{}")
            .map_err(map_err)?;
        Ok(())
    }

    /// Consumes the server-sent event stream until `on_message` returns false
    /// or the connection drops.
    pub fn stream(&self, mut on_message: impl FnMut(StreamMessage) -> bool) -> Result<()> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(500))
            .timeout_read(Duration::from_secs(45))
            .build();
        let resp = agent
            .get(&format!("{}/v1/stream", self.base))
            .set("Authorization", &self.auth)
            .set("Accept", "text/event-stream")
            .call()
            .map_err(map_err)?;
        let reader = BufReader::new(resp.into_reader());
        let mut data = String::new();
        for line in reader.lines() {
            let line = line.map_err(|e| ClientError::Http(e.to_string()))?;
            if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim_start());
            } else if line.is_empty() && !data.is_empty() {
                if let Ok(msg) = serde_json::from_str::<StreamMessage>(&data) {
                    if !on_message(msg) {
                        return Ok(());
                    }
                }
                data.clear();
            }
        }
        Ok(())
    }
}
