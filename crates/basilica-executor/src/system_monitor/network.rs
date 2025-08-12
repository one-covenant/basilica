//! Network monitoring functionality

use super::types::{NetworkInfo, NetworkInterface};
use anyhow::Result;
use std::time::Instant;
use sysinfo::Networks;

/// Network monitoring handler
#[derive(Debug)]
pub struct NetworkMonitor {
    networks: Networks,
    last_measurement: Option<(Instant, u64, u64)>, // (time, tx_bytes, rx_bytes)
}

impl NetworkMonitor {
    /// Create new network monitor
    pub fn new() -> Self {
        let networks = Networks::new_with_refreshed_list();
        Self {
            networks,
            last_measurement: None,
        }
    }

    /// Get network information
    pub async fn get_network_info(&self) -> Result<NetworkInfo> {
        let mut interfaces = Vec::new();
        let mut total_sent = 0;
        let mut total_received = 0;

        for (interface_name, network) in &self.networks {
            let interface_info = NetworkInterface {
                name: interface_name.clone(),
                bytes_sent: network.total_transmitted(),
                bytes_received: network.total_received(),
                packets_sent: network.total_packets_transmitted(),
                packets_received: network.total_packets_received(),
                errors_sent: network.total_errors_on_transmitted(),
                errors_received: network.total_errors_on_received(),
                is_up: true,
            };

            total_sent += interface_info.bytes_sent;
            total_received += interface_info.bytes_received;
            interfaces.push(interface_info);
        }

        Ok(NetworkInfo {
            interfaces,
            total_bytes_sent: total_sent,
            total_bytes_received: total_received,
        })
    }

    /// Refresh network data
    pub fn refresh(&mut self) {
        self.networks.refresh();
    }

    /// Calculate current network bandwidth in Mbps based on time-delta measurements
    pub fn calculate_bandwidth_mbps(&mut self) -> f64 {
        self.networks.refresh();

        let current_time = Instant::now();
        let mut total_tx = 0u64;
        let mut total_rx = 0u64;

        for (_, network) in &self.networks {
            total_tx += network.total_transmitted();
            total_rx += network.total_received();
        }

        if self.last_measurement.is_none() {
            self.last_measurement = Some((current_time, total_tx, total_rx));
            return 0.0;
        }

        let (last_time, last_tx, last_rx) = self.last_measurement.unwrap();
        let time_delta = current_time.duration_since(last_time).as_secs_f64();

        if time_delta == 0.0 {
            return 0.0;
        }

        let bytes_delta = (total_tx - last_tx) + (total_rx - last_rx);
        let bits_per_second = (bytes_delta as f64 * 8.0) / time_delta;
        let mbps = bits_per_second / 1_000_000.0;

        self.last_measurement = Some((current_time, total_tx, total_rx));

        mbps
    }
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}
