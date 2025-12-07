use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn start_mock_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().unwrap();

    let handle = thread::spawn(move || {
        for incoming in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = incoming.peek(&mut buf);
            let mut parts = incoming.try_clone().unwrap();
            let response = if buf.windows(7).any(|w| w == b"/status") {
                let body = r#"{
                    "healthy": true,
                    "load_avg_1m": 0.5,
                    "load_avg_5m": 0.4,
                    "load_avg_15m": 0.3,
                    "uptime_seconds": 60,
                    "last_scrape_unix_ms": 123,
                    "last_errors": [],
                    "node_power_watts": 100.0,
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
                    "disk_root_used_bytes": 100,
                    "disk_root_io_time_ms": 1,
                    "primary_nic": "eth0",
                    "net_rx_bytes_per_sec": 10.0,
                    "net_tx_bytes_per_sec": 5.0,
                    "net_drops_per_sec": 0.0
                }"#;
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else if buf.windows(8).any(|w| w == b"/metrics") {
                let body = "# HELP dummy dummy\n# TYPE dummy counter\ndummy 1\n";
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string()
            };
            let _ = parts.write_all(response.as_bytes());
        }
    });

    (format!("127.0.0.1:{}", addr.port()), handle)
}

#[test]
fn cli_status_reads_mock_server() {
    let (addr, handle) = start_mock_server();
    let mut cmd = Command::cargo_bin("esnode-core").unwrap();
    cmd.args(["--listen-address", &addr, "status"])
        .assert()
        .success()
        .stdout(contains("Node status"));
    handle.thread().unpark();
}

#[test]
fn cli_metrics_fetches_mock_metrics() {
    let (addr, handle) = start_mock_server();
    let mut cmd = Command::cargo_bin("esnode-core").unwrap();
    cmd.args(["--listen-address", &addr, "metrics"])
        .assert()
        .success()
        .stdout(contains("dummy"));
    handle.thread().unpark();
}

#[test]
fn cli_enable_metric_set_persists_config() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("esnode.toml");

    Command::cargo_bin("esnode-core")
        .unwrap()
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "enable-metric-set",
            "host",
        ])
        .assert()
        .success();

    let contents = fs::read_to_string(&config_path).unwrap();
    assert!(contents.contains("enable_cpu = true"));
    assert!(contents.contains("enable_network = true"));
}
