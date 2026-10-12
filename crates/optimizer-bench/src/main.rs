//! Repeatable native-core and simple HTTP performance checks, written entirely in Rust.
//! Does not need Node.js or a third-party load-test framework.
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use optimizer_core::{parse_request, run_request, JobRequest};
use serde_json::{json, Value};

#[derive(Debug)]
struct Args {
    fixture: String,
    iterations: usize,
    warmup: usize,
    concurrency: usize,
    http: Option<SocketAddr>,
    output: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        fixture: "examples/system-check.json".into(),
        iterations: 10,
        warmup: 2,
        concurrency: 1,
        http: None,
        output: None,
    };
    let mut iter = env::args().skip(1);
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| format!("missing value after {flag}"))?;
        match flag.as_str() {
            "--fixture" => args.fixture = value,
            "--iterations" => args.iterations = value.parse().map_err(|_| "--iterations requires integer")?,
            "--warmup" => args.warmup = value.parse().map_err(|_| "--warmup requires integer")?,
            "--concurrency" => args.concurrency = value.parse().map_err(|_| "--concurrency requires integer")?,
            "--http" => args.http = Some(value.parse().map_err(|_| "--http requires IP:PORT (HTTP only)")?),
            "--output" => args.output = Some(value),
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if !(1..=10_000).contains(&args.iterations) || args.warmup > 1_000 {
        return Err("iterations must be 1..=10000 and warmup <= 1000".into());
    }
    if !(1..=64).contains(&args.concurrency) { return Err("concurrency must be 1..=64".into()); }
    Ok(args)
}

fn elapsed_us(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn bench_core(request: &JobRequest, iterations: usize, warmup: usize) -> Result<(Vec<u64>, usize, Duration), String> {
    for _ in 0..warmup { run_request(request.clone()).map_err(|e| e.to_string())?; }
    let mut micros = Vec::with_capacity(iterations);
    let start_wall = Instant::now();
    for _ in 0..iterations {
        let start = Instant::now();
        // Move a distinct request into each run, reflecting real API use.
        let result = run_request(request.clone()).map_err(|e| e.to_string())?;
        std::hint::black_box(result);
        micros.push(elapsed_us(start));
    }
    Ok((micros, iterations, start_wall.elapsed()))
}

fn request_http(addr: SocketAddr, body: &[u8], token: Option<&str>) -> Result<u64, String> {
    let start = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(120))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(15))).map_err(|e| e.to_string())?;
    let authorization = token.map_or(String::new(), |secret| format!("Authorization: Bearer {secret}\r\n"));
    let header = format!(
        "POST /api/v1/jobs HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nConnection: close\r\n{authorization}Content-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).map_err(|e| format!("write headers: {e}"))?;
    stream.write_all(body).map_err(|e| format!("write body: {e}"))?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|e| format!("read response: {e}"))?;
    let line_end = response.windows(2).position(|v| v == b"\r\n").ok_or("no HTTP status line")?;
    let first_line = std::str::from_utf8(&response[..line_end]).map_err(|e| e.to_string())?;
    if !first_line.contains(" 200 ") {
        return Err(format!("HTTP request failed: {first_line}"));
    }
    Ok(elapsed_us(start))
}

fn bench_http(addr: SocketAddr, body: Vec<u8>, iterations: usize, concurrency: usize) -> (Vec<u64>, usize, Duration) {
    let start_wall = Instant::now();
    let cursor = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(Mutex::new(Vec::<u64>::new()));
    let failures = Arc::new(AtomicUsize::new(0));
    let payload = Arc::new(body);
    let token = env::var("OPTIMIZER_API_KEY").ok();
    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let cursor = Arc::clone(&cursor);
        let results = Arc::clone(&results);
        let failures = Arc::clone(&failures);
        let payload = Arc::clone(&payload);
        let token = token.clone();
        handles.push(thread::spawn(move || {
            loop {
                let index = cursor.fetch_add(1, Ordering::Relaxed);
                if index >= iterations { break; }
                match request_http(addr, payload.as_ref(), token.as_deref()) {
                    Ok(duration) => {
                        if let Ok(mut times) = results.lock() { times.push(duration); }
                    }
                    Err(error) => {
                        eprintln!("HTTP request {}: {}", index + 1, error);
                        failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    for handle in handles { if handle.join().is_err() { failures.fetch_add(1, Ordering::Relaxed); } }
    let times = match Arc::try_unwrap(results) {
        Ok(mutex) => mutex.into_inner().unwrap_or_default(),
        Err(shared) => shared.lock().map_or_else(|_| Vec::new(), |v| v.clone()),
    };
    (times, failures.load(Ordering::Relaxed), start_wall.elapsed())
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() { return 0; }
    let rank = ((sorted.len() * percentile) + 99) / 100;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn linux_peak_rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn statistics(mut samples: Vec<u64>, failures: usize, mode: &str, fixture: &str,
              attempted: usize, wall: Duration) -> Value {
    samples.sort_unstable();
    let successful = samples.len();
    let sum: u128 = samples.iter().map(|&value| value as u128).sum();
    let average = if successful > 0 { sum / successful as u128 } else { 0 };
    json!({
        "mode": mode, "fixture": fixture,
        "attempted": attempted, "successful": successful, "failed": failures,
        "wall_ms": wall.as_millis().to_string(),
        "benchmark_process_peak_rss_kb_linux": linux_peak_rss_kb(),
        "throughput_requests_per_second": if wall.as_secs_f64() > 0.0 { (successful as f64) / wall.as_secs_f64() } else { 0.0 },
        "latency_us": {
            "minimum": samples.first().copied(),
            "p50": if successful > 0 { Some(percentile(&samples, 50)) } else { None },
            "p95": if successful > 0 { Some(percentile(&samples, 95)) } else { None },
            "maximum": samples.last().copied(),
            "mean": if successful > 0 { Some(average.to_string()) } else { None }
        }
    })
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let source = fs::read(&args.fixture).map_err(|e| format!("fixture {}: {e}", args.fixture))?;
    let parsed = parse_request(std::str::from_utf8(&source).map_err(|e| e.to_string())?)
        .map_err(|e| format!("invalid benchmark fixture: {e}"))?;
    let (times, failures, mode, wall) = if let Some(addr) = args.http {
        let (times, failures, wall) = bench_http(addr, source, args.iterations, args.concurrency);
        (times, failures, "http", wall)
    } else {
        let (times, count, wall) = bench_core(&parsed, args.iterations, args.warmup)?;
        (times, args.iterations.saturating_sub(count), "core", wall)
    };
    let report = statistics(times, failures, mode, &args.fixture, args.iterations, wall);
    let output = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    println!("{output}");
    if let Some(path) = args.output {
        fs::write(&path, format!("{output}\n")).map_err(|e| format!("write {path}: {e}"))?;
    }
    if report["failed"].as_u64().unwrap_or(1) > 0 { return Err("some requests failed".into()); }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("benchmark failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles_are_nearest_rank() {
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 50), 3);
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 95), 5);
        assert_eq!(percentile(&[], 50), 0);
    }
    #[test]
    fn report_has_no_fictitious_numbers() {
        let output = statistics(vec![100, 200, 300], 0, "core", "test.json", 3, Duration::from_millis(1));
        assert_eq!(output["latency_us"]["p50"], 200);
        assert_eq!(output["attempted"], 3);
    }
}
