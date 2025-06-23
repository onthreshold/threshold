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
