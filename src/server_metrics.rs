//! Reads the *microtak-server process's own* metrics -- distinct from the
//! host-wide CPU/RAM/disk in [`crate::system`]. The server already exposes
//! everything over its Prometheus endpoint (`GET /metrics`, plain HTTP,
//! unauthenticated -- meant for a private scrape, see microtak-server's
//! `src/metrics.rs`): its resident memory and CPU time come from the
//! server's `/proc/self`, so these are the server binary's figures, not the
//! box's. We scrape twice a few hundred ms apart to turn the cumulative CPU
//! counter into a live "cores busy right now" reading.
//!
//! This talks plain HTTP to a private address (typically `127.0.0.1`), not
//! the mTLS Marti API [`crate::client`] uses -- a separate, tiny client.

use std::time::Duration;

/// Gap between the two scrapes used to derive instantaneous CPU use. Short
/// enough to barely delay a page load, long enough to be a stable sample.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(400);

/// A point-in-time snapshot of the microtak-server process + its live
/// operational counters, parsed from the Prometheus exposition.
#[derive(Debug, Default, Clone)]
pub struct ServerMetrics {
    pub version: Option<String>,
    /// Resident memory of the server process, bytes (`process_resident_memory_bytes`).
    pub resident_bytes: f64,
    /// Cumulative CPU time the server has used, seconds (`process_cpu_seconds_total`).
    pub cpu_seconds: f64,
    /// CPU cores the server is using *right now*, from two scrapes a moment
    /// apart (`None` if only one scrape succeeded). 1.0 == one core saturated.
    pub cpu_cores_used: Option<f64>,
    pub connected_tls: u64,
    pub connected_tcp: u64,
    pub open_connections: u64,
    pub devices: u64,
    pub missions: u64,
    pub groups: u64,
    pub cot_events_tls: u64,
    pub cot_events_tcp: u64,
    pub enrollments: u64,
    pub data_dir_bytes: u64,
    pub content_bytes: u64,
}

impl ServerMetrics {
    pub fn connected_total(&self) -> u64 {
        self.connected_tls + self.connected_tcp
    }

    pub fn cot_events_total(&self) -> u64 {
        self.cot_events_tls + self.cot_events_tcp
    }
}

/// Fetch and parse the server's metrics. `base` is the metrics endpoint's
/// base URL, e.g. `http://127.0.0.1:9090`. Scrapes twice to derive live CPU.
pub async fn fetch(base: &str) -> Result<ServerMetrics, reqwest::Error> {
    // A plain, proxy-free client: the endpoint is a private loopback address,
    // never something a forward proxy should see.
    let http = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("{}/metrics", base.trim_end_matches('/'));

    let first = http.get(&url).send().await?.error_for_status()?;
    let first_at = std::time::Instant::now();
    let first_body = first.text().await?;
    let mut metrics = parse(&first_body);

    // Second scrape for an instantaneous CPU reading. A failure here is
    // non-fatal: we still have memory + the cumulative counter.
    tokio::time::sleep(CPU_SAMPLE_GAP).await;
    if let Ok(resp) = http.get(&url).send().await
        && let Ok(resp) = resp.error_for_status()
    {
        let elapsed = first_at.elapsed().as_secs_f64();
        if let Ok(body) = resp.text().await
            && elapsed > 0.0
        {
            let second = parse(&body);
            let delta = second.cpu_seconds - metrics.cpu_seconds;
            if delta >= 0.0 {
                metrics.cpu_cores_used = Some(delta / elapsed);
                metrics.cpu_seconds = second.cpu_seconds;
            }
        }
    }

    Ok(metrics)
}

/// Parse a Prometheus text exposition into [`ServerMetrics`]. Tolerant: it
/// reads the samples it knows and ignores everything else, so a new metric
/// upstream never breaks it.
pub fn parse(body: &str) -> ServerMetrics {
    let mut m = ServerMetrics::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `name{labels} value` or `name value`: the value is the last token,
        // the rest is the sample identifier.
        let Some((ident, value_str)) = line.rsplit_once(char::is_whitespace) else {
            continue;
        };
        let Ok(value) = value_str.trim().parse::<f64>() else {
            continue;
        };
        let (name, labels) = match ident.split_once('{') {
            Some((name, rest)) => (name, rest.trim_end_matches('}')),
            None => (ident, ""),
        };

        match name {
            "microtak_build_info" => {
                m.version = label_value(labels, "version");
            }
            "process_resident_memory_bytes" => m.resident_bytes = value,
            "process_cpu_seconds_total" => m.cpu_seconds = value,
            "microtak_open_connections" => m.open_connections = value as u64,
            "microtak_devices" => m.devices = value as u64,
            "microtak_missions" => m.missions = value as u64,
            "microtak_groups" => m.groups = value as u64,
            "microtak_enrollments_total" => m.enrollments = value as u64,
            "microtak_data_dir_bytes" => m.data_dir_bytes = value as u64,
            "microtak_content_bytes" => m.content_bytes = value as u64,
            "microtak_connected_clients" => match label_value(labels, "transport").as_deref() {
                Some("tls") => m.connected_tls = value as u64,
                Some("tcp") => m.connected_tcp = value as u64,
                _ => {}
            },
            "microtak_cot_events_received_total" => {
                match label_value(labels, "transport").as_deref() {
                    Some("tls") => m.cot_events_tls = value as u64,
                    Some("tcp") => m.cot_events_tcp = value as u64,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    m
}

/// Pull `key="value"` out of a Prometheus label set.
fn label_value(labels: &str, key: &str) -> Option<String> {
    labels.split(',').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k.trim() == key {
            Some(v.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# HELP microtak_build_info Server version.
# TYPE microtak_build_info gauge
microtak_build_info{version="0.17.0"} 1
# HELP microtak_devices Enrolled device identities.
# TYPE microtak_devices gauge
microtak_devices 4
microtak_missions 2
microtak_groups 3
# TYPE microtak_connected_clients gauge
microtak_connected_clients{transport="tls"} 2
microtak_connected_clients{transport="tcp"} 0
microtak_open_connections 2
microtak_cot_events_received_total{transport="tls"} 1500
microtak_cot_events_received_total{transport="tcp"} 0
microtak_enrollments_total 7
microtak_data_dir_bytes 1048576
microtak_content_bytes 524288
process_cpu_seconds_total 12.34
process_resident_memory_bytes 20971520
"#;

    #[test]
    fn parses_known_samples_and_ignores_the_rest() {
        let m = parse(SAMPLE);
        assert_eq!(m.version.as_deref(), Some("0.17.0"));
        assert_eq!(m.devices, 4);
        assert_eq!(m.missions, 2);
        assert_eq!(m.groups, 3);
        assert_eq!(m.connected_tls, 2);
        assert_eq!(m.connected_tcp, 0);
        assert_eq!(m.connected_total(), 2);
        assert_eq!(m.open_connections, 2);
        assert_eq!(m.cot_events_tls, 1500);
        assert_eq!(m.cot_events_total(), 1500);
        assert_eq!(m.enrollments, 7);
        assert_eq!(m.data_dir_bytes, 1048576);
        assert_eq!(m.content_bytes, 524288);
        assert_eq!(m.cpu_seconds, 12.34);
        assert_eq!(m.resident_bytes, 20971520.0);
        assert!(m.cpu_cores_used.is_none());
    }

    #[test]
    fn empty_body_is_all_zeroes_not_a_panic() {
        let m = parse("");
        assert_eq!(m.devices, 0);
        assert_eq!(m.resident_bytes, 0.0);
        assert!(m.version.is_none());
    }

    #[test]
    fn label_value_extracts_the_right_key() {
        assert_eq!(
            label_value("transport=\"tls\",foo=\"bar\"", "transport").as_deref(),
            Some("tls")
        );
        assert_eq!(label_value("transport=\"tls\"", "missing"), None);
    }
}
