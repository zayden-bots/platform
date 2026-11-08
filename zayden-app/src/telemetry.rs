use std::net::SocketAddr;

use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

impl LogFormat {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("LOG_FORMAT") {
            Ok(v) if v.eq_ignore_ascii_case("json") => Self::Json,
            _ => Self::Text,
        }
    }
}

pub fn install_metrics(service: &'static str, default_addr: SocketAddr) {
    let addr = match std::env::var("METRICS_BIND_ADDR") {
        Ok(v) if v.eq_ignore_ascii_case("off") => {
            info!("metrics exporter disabled");
            return;
        },
        Ok(v) => v.parse().unwrap_or_else(|e| {
            warn!(value = %v, error = ?e, "invalid METRICS_BIND_ADDR; using default");
            default_addr
        }),
        Err(_) => default_addr,
    };

    match PrometheusBuilder::new()
        .with_http_listener(addr)
        .add_global_label("service", service)
        .install()
    {
        Ok(()) => {
            metrics::gauge!("zayden_build_info", "version" => env!("CARGO_PKG_VERSION"))
                .set(1.0);
            info!(%addr, "metrics exporter listening");
        },
        Err(e) => warn!(error = ?e, "failed to start metrics exporter"),
    }
}
