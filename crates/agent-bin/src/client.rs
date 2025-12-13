// ESNODE | Source Available BUSL-1.1 | Copyright (c) 2024 Estimatedstocks AB
use agent_core::state::StatusSnapshot;
use anyhow::{Context, Result};

/// Lightweight HTTP client for talking to the local agent without external deps.
pub struct AgentClient {
    base_url: String,
}

impl AgentClient {
    pub fn new(listen_address: &str) -> Self {
        let normalized =
            if listen_address.starts_with("http://") || listen_address.starts_with("https://") {
                listen_address.to_string()
            } else {
                format!("http://{}", listen_address)
            };
        AgentClient {
            base_url: normalized.trim_end_matches('/').to_string(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn fetch_status(&self) -> Result<StatusSnapshot> {
        let url = format!("{}/status", self.base_url);
        let body = ureq::get(&url)
            .call()
            .context("requesting /status")?
            .into_string()
            .context("reading /status body")?;
        let snapshot: StatusSnapshot =
            serde_json::from_str(&body).context("parsing status JSON")?;
        Ok(snapshot)
    }

    pub fn fetch_metrics_text(&self) -> Result<String> {
        let url = format!("{}/metrics", self.base_url);
        let body = ureq::get(&url)
            .call()
            .context("requesting /metrics")?
            .into_string()
            .context("reading /metrics body")?;
        Ok(body)
    }

    pub fn fetch_orchestrator_metrics(&self) -> Result<esnode_orchestrator::PubMetrics> {
        let url = format!("{}/orchestrator/metrics", self.base_url);
        let resp = ureq::get(&url).call();
        match resp {
            Ok(r) => {
                let body = r
                    .into_string()
                    .context("reading /orchestrator/metrics body")?;
                let metrics: esnode_orchestrator::PubMetrics =
                    serde_json::from_str(&body).context("parsing orchestrator metrics")?;
                Ok(metrics)
            }
            Err(ureq::Error::Status(404, _)) => {
                // If 404, it means orchestrator is disabled. Return default/empty.
                Ok(esnode_orchestrator::PubMetrics {
                    device_count: 0,
                    pending_tasks: 0,
                    devices: vec![],
                })
            }
            Err(e) => Err(anyhow::anyhow!("request failed: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AgentClient;
    use agent_core::state::StatusSnapshot;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn base_url_normalizes_without_scheme() {
        let c = AgentClient::new("localhost:9100");
        assert_eq!(c.base_url(), "http://localhost:9100");
    }

    #[test]
    fn base_url_keeps_scheme() {
        let c = AgentClient::new("https://example.com");
        assert_eq!(c.base_url(), "https://example.com");
    }

    #[test]
    fn fetch_status_reads_json_from_local_server() {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(_) => {
                // Environment may disallow binding; skip in that case.
                return;
            }
        };
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut _buf = [0u8; 1024];
                let _ = stream.read(&mut _buf);
                let body = r#"{
                    "healthy": true,
                    "load_avg_1m": 0.1,
                    "load_avg_5m": 0.1,
                    "load_avg_15m": 0.1,
                    "uptime_seconds": 5,
                    "last_scrape_unix_ms": 1,
                    "last_errors": [],
                    "node_power_watts": null,
                    "cpu_package_power_watts": [],
                    "cpu_temperatures": [],
                    "gpus": [],
                    "cpu_cores": 4,
                    "cpu_util_percent": 10.0,
                    "mem_total_bytes": 1024,
                    "mem_used_bytes": 512,
                    "mem_free_bytes": 256,
                    "swap_used_bytes": 0,
                    "disk_root_total_bytes": 1024,
                    "disk_root_used_bytes": 512,
                    "disk_root_io_time_ms": 1,
                    "primary_nic": "eth0",
                    "net_rx_bytes_per_sec": 10.0,
                    "net_tx_bytes_per_sec": 5.0,
                    "net_drops_per_sec": 0.0
                }"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let client = AgentClient::new(&format!("{}", addr));
        let snapshot: StatusSnapshot = client.fetch_status().expect("status json");
        assert!(snapshot.healthy);
        assert_eq!(snapshot.cpu_cores, Some(4));
        assert_eq!(snapshot.cpu_util_percent, Some(10.0));
    }
}
