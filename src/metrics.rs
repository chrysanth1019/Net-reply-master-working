use crate::conf;

use conf::parse_args;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response};
use prometheus::Encoder;
use prometheus::TextEncoder;
use prometheus::{Counter, IntGauge, Registry};
use std::net::{IpAddr, SocketAddrV4};
use std::str::FromStr;
use std::{error::Error, sync::Arc};

// Metrics and Observability
pub struct Metrics {
    pub slave_active_connections: IntGauge,
    pub slave_total_connections: Counter,
    pub slave_disconnections: Counter,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            slave_active_connections: IntGauge::new(
                "slave_active_connections",
                "Current number of active slave connections",
            )
            .unwrap(),

            slave_total_connections: Counter::new(
                "slave_total_connections",
                "Total number of slave connections made",
            )
            .unwrap(),

            slave_disconnections: Counter::new(
                "slave_disconnections",
                "Total number of slave disconnections",
            )
            .unwrap(),
        }
    }

    pub fn register(&self, registry: &Registry) {
        registry
            .register(Box::new(self.slave_active_connections.clone()))
            .unwrap();
        registry
            .register(Box::new(self.slave_total_connections.clone()))
            .unwrap();
        registry
            .register(Box::new(self.slave_disconnections.clone()))
            .unwrap();
    }
}

pub async fn start_metrics_server(
    registry: Arc<Registry>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let make_svc = make_service_fn(move |_| {
        let registry = registry.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |_req| {
                let registry = registry.clone();
                async move {
                    // Collect metrics into a string
                    let mut buffer = Vec::new();
                    let encoder = TextEncoder::new();
                    encoder.encode(&registry.gather(), &mut buffer).unwrap();

                    // Format metrics into a JavaScript-driven live dashboard
                    let metrics = String::from_utf8(buffer).unwrap();
                    let html = format!(
                        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Metrics Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        h1 {{ color: #2c3e50; }}
        pre {{ background: #ecf0f1; padding: 15px; border-radius: 5px; overflow-x: auto; }}
        .timestamp {{ color: #7f8c8d; font-size: 0.9em; }}
    </style>
</head>
<body>
    <h1>Metrics Dashboard</h1>
    <p class="timestamp">Last updated: <span id="timestamp"></span></p>
    <pre id="metrics">{}</pre>
    <script>
        async function fetchMetrics() {{
            try {{
                const response = await fetch(window.location.href);
                const text = await response.text();
                const parser = new DOMParser();
                const doc = parser.parseFromString(text, 'text/html');
                const metrics = doc.querySelector('pre').innerText;

                document.getElementById('metrics').innerText = metrics;
                document.getElementById('timestamp').innerText = new Date().toLocaleTimeString();
            }} catch (err) {{
                console.error('Failed to fetch metrics:', err);
            }}
        }}

        // Refresh every 5 seconds
        setInterval(fetchMetrics, 5000);
        // Initial timestamp
        document.getElementById('timestamp').innerText = new Date().toLocaleTimeString();
    </script>
</body>
</html>"#,
                        metrics
                    );

                    Ok::<_, hyper::Error>(Response::new(Body::from(html)))
                }
            }))
        }
    });
    let config = parse_args();
    let addr_parts: Vec<&str> = config.metrics_addr.split(':').collect();

    let mut addr = ([0, 0, 0, 0], 9091).into();
    if addr_parts.len() == 2 {
        let ip_str = addr_parts[0];
        let port_str = addr_parts[1];
        if let Ok(ip_addr) = IpAddr::from_str(ip_str) {
            if let IpAddr::V4(ipv4_addr) = ip_addr {
                let port: u16 = port_str.parse().expect("Invalid port number");
                let socket_addr = SocketAddrV4::new(ipv4_addr, port);
                println!("Socket Address: {:?}", socket_addr);
                addr = (ipv4_addr.octets(), port).into();
            }
        }
    }

    let server = hyper::Server::bind(&addr).serve(make_svc);

    println!("Metrics server running on http://{}", addr);
    server.await?;
    Ok(())
}
