#[macro_export]
macro_rules! route_metrics {
    ($route:expr, $body:expr) => {{
        metrics::counter!("grpc_requests_total", "route" => $route.to_string()).increment(1);
        let start = std::time::Instant::now();
        let result = ($body).await;
        let elapsed = start.elapsed().as_secs_f64();
        metrics::histogram!("grpc_response_time_seconds", "route" => $route.to_string()).record(elapsed);
        if result.is_err() {
            metrics::counter!("grpc_errors_total", "route" => $route.to_string()).increment(1);
        }
        result
    }};
}

#[macro_export]
macro_rules! broadcast_sent_metrics {
    ($message_type:expr) => {{
        metrics::counter!("broadcast_sent", "type" => $message_type.to_string()).increment(1);
    }};
}

#[macro_export]
macro_rules! broadcast_received_metrics {
    ($message_type:expr) => {{
        metrics::counter!("broadcast_received", "type" => $message_type.to_string()).increment(1);
    }};
}

#[macro_export]
macro_rules! current_round_metrics {
    ($round:expr, $node_id:expr) => {{
        metrics::gauge!("current_round", "node_id" => $node_id.to_string()).set(f64::from($round));
    }};
}
