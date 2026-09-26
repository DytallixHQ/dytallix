use crate::risk::pulseguard::bench::latency::quick_bench;#[test]fn bench_under_target(){ let (_p50,p95,_p99)=quick_bench(); assert!(p95 < 100.0, "P95 latency {} ms exceeds target", p95); }
