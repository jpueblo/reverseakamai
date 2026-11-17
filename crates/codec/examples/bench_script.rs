//! Offline key/version latency probe. Inputs are local files in a JSON manifest.
use akamai_codec::script::{extract_bmak_shuffle_key, extract_bmak_ver};
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};
fn main() {
    let manifest = std::env::args().nth(1).expect("local manifest path");
    let mode = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "report".to_owned());
    assert!(
        matches!(mode.as_str(), "report" | "pair"),
        "mode must be report or pair"
    );
    let entries: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let mut results = Vec::new();
    for case in entries.as_array().unwrap() {
        let source = std::fs::read_to_string(case["path"].as_str().unwrap()).unwrap();
        let n = case["iterations"].as_u64().unwrap() as usize;
        assert!(n > 0, "iterations must be positive");
        let run = || {
            if mode == "report" {
                let report = akamai_codec::script::analyze_script(black_box(&source));
                (report.shuffle_key, report.version_token)
            } else {
                (
                    extract_bmak_shuffle_key(black_box(&source)),
                    extract_bmak_ver(black_box(&source)),
                )
            }
        };
        let expected = run();
        for _ in 0..3 {
            black_box(run());
        }
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let start = Instant::now();
            let actual = black_box(run());
            times.push(start.elapsed().as_nanos() as u64);
            assert_eq!(actual, expected, "extraction must be deterministic");
        }
        times.sort_unstable();
        results.push(json!({"label":case["label"],"bytes":source.len(),"iterations":n,"median_us":times[n/2] as f64/1000.0,"p95_us":times[((n*95)/100).min(n-1)] as f64/1000.0,"mean_us":times.iter().map(|t|*t as f64).sum::<f64>()/n as f64/1000.0,"key":expected.0,"version_present":expected.1.is_some()}));
    }
    println!(
        "{}",
        json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"mode":mode,"cases":results,"network_requests":0})
    );
}
