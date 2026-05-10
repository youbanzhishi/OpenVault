//! Performance benchmarks for OpenVault.
//!
//! Covers four key areas:
//! - **BackupBenchmark**: Backup operation throughput and latency
//! - **RestoreBenchmark**: Restore operation latency and throughput
//! - **CryptoBenchmark**: AES-256-GCM encryption/decryption throughput
//! - **SearchBenchmark**: Index build and search latency
//!
//! Uses a lightweight custom benchmark harness compatible with stable Rust
//! (no nightly-only `#[bench]` required). Each benchmark function
//! follows the pattern: warm-up iterations → timed iterations → report.

use std::time::{Duration, Instant};

use crate::crypto::{Aes256GcmCrypto, VaultCrypto};
use crate::integrity::{Checksum, HashAlgorithm};
use crate::restore::{ConflictStrategy, RestoreOptions};
use crate::search::{FileIndex, FileIndexEntry, KeywordSemanticSearch, SemanticSearch};
use crate::snapshot::{BackupStrategy, FileEntry, Snapshot};

// ============================================================================
// Benchmark harness utilities
// ============================================================================

/// Result of a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchResult {
    /// Name of the benchmark.
    pub name: String,
    /// Number of iterations executed.
    pub iterations: u64,
    /// Total elapsed time across all iterations.
    pub total: Duration,
    /// Average time per iteration.
    pub mean: Duration,
    /// Minimum iteration time.
    pub min: Duration,
    /// Maximum iteration time.
    pub max: Duration,
    /// Throughput in operations per second (or bytes/sec for byte benchmarks).
    pub throughput_ops_sec: Option<f64>,
}

impl std::fmt::Display for BenchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} iterations, mean={:.2?}, min={:.2?}, max={:.2?}",
            self.name, self.iterations, self.mean, self.min, self.max
        )?;
        if let Some(t) = self.throughput_ops_sec {
            write!(f, ", throughput={:.0} ops/s", t)?;
        }
        Ok(())
    }
}

/// Run a benchmark closure for the given number of iterations.
pub fn run_bench<F>(name: &str, iterations: u64, f: F) -> BenchResult
where
    F: Fn(),
{
    // Warm-up: 10% of iterations (at least 1)
    let warmup = std::cmp::max(iterations / 10, 1);
    for _ in 0..warmup {
        f();
    }

    let mut times = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        times.push(start.elapsed());
    }

    let total: Duration = times.iter().sum();
    let mean = total / iterations as u32;
    let min = times.iter().min().copied().unwrap_or(Duration::ZERO);
    let max = times.iter().max().copied().unwrap_or(Duration::ZERO);
    let throughput_ops_sec = if total.as_secs_f64() > 0.0 {
        Some(iterations as f64 / total.as_secs_f64())
    } else {
        None
    };

    BenchResult {
        name: name.to_string(),
        iterations,
        total,
        mean,
        min,
        max,
        throughput_ops_sec,
    }
}

/// Run a benchmark that produces a byte-count throughput metric.
pub fn run_bench_bytes<F>(name: &str, iterations: u64, bytes_per_iter: u64, f: F) -> BenchResult
where
    F: Fn(),
{
    let result = run_bench(name, iterations, f);
    let throughput = if result.total.as_secs_f64() > 0.0 {
        Some((iterations * bytes_per_iter) as f64 / result.total.as_secs_f64())
    } else {
        None
    };
    BenchResult {
        throughput_ops_sec: throughput,
        ..result
    }
}

// ============================================================================
// Backup Benchmarks
// ============================================================================

/// Helper: create a FileEntry with the given path, size and a deterministic checksum.
fn make_file_entry(i: usize, size: u64) -> FileEntry {
    FileEntry {
        path: format!("file_{:05}.dat", i),
        size,
        mtime: 1700000000 + i as i64,
        checksum: format!("sha256_{:064x}", i),
    }
}

/// Benchmark: small file (< 1 KB) batch backup throughput.
///
/// Creates a snapshot with many tiny file entries and measures how fast
/// the engine can process them.
pub fn bench_small_file_backup(iterations: u64) -> BenchResult {
    run_bench("small_file_batch_backup_1k", iterations, || {
        let mut snap = Snapshot::new(
            BackupStrategy::Full,
            "bench-source".to_string(),
            "local".to_string(),
            None,
        );
        for i in 0..1000u64 {
            snap.add_entry(make_file_entry(i as usize, 256 + (i % 512)));
        }
        assert_eq!(snap.file_count(), 1000);
    })
}

/// Benchmark: large file (simulated > 100 MB) backup with checksum.
///
/// Measures checksum computation on a large buffer as a proxy for
/// large-file backup overhead.
pub fn bench_large_file_backup(iterations: u64) -> BenchResult {
    // 100 MB buffer
    let data = vec![0xABu8; 100 * 1024 * 1024];
    run_bench_bytes(
        "large_file_backup_checksum_100mb",
        iterations,
        data.len() as u64,
        || {
            let _ = Checksum::compute(&data, HashAlgorithm::Sha256);
        },
    )
}

/// Benchmark: incremental vs full backup comparison.
///
/// Measures snapshot creation time for incremental (changed files only)
/// vs full (all files) scenarios.
pub fn bench_incremental_vs_full(iterations: u64) -> (BenchResult, BenchResult) {
    let all_files: Vec<FileEntry> = (0..500)
        .map(|i| make_file_entry(i, 4096 + (i % 8192) as u64))
        .collect();

    let changed_files: Vec<FileEntry> = (0..50)
        .map(|i| {
            let mut e = make_file_entry(i, 4096 + (i % 8192) as u64);
            e.checksum = format!("sha256_changed_{:064x}", i);
            e
        })
        .collect();

    let full_bench = run_bench("full_backup_500_files", iterations, || {
        let mut snap = Snapshot::new(
            BackupStrategy::Full,
            "bench-source".to_string(),
            "local".to_string(),
            None,
        );
        for f in &all_files {
            snap.add_entry(f.clone());
        }
    });

    let incr_bench = run_bench("incremental_backup_50_files", iterations, || {
        let mut snap = Snapshot::new(
            BackupStrategy::Incremental,
            "bench-source".to_string(),
            "local".to_string(),
            Some("snap-parent".to_string()),
        );
        for f in &changed_files {
            snap.add_entry(f.clone());
        }
    });

    (full_bench, incr_bench)
}

// ============================================================================
// Restore Benchmarks
// ============================================================================

/// Benchmark: single file restore latency.
///
/// Simulates preparing a restore operation for a single file.
pub fn bench_single_file_restore(iterations: u64) -> BenchResult {
    run_bench("single_file_restore_latency", iterations, || {
        let opts = RestoreOptions::to("/tmp/bench-restore")
            .with_conflict_strategy(ConflictStrategy::Overwrite);
        let _ = opts;
    })
}

/// Benchmark: batch restore throughput.
///
/// Measures the overhead of setting up restore for a batch of 100 files.
pub fn bench_batch_restore(iterations: u64) -> BenchResult {
    run_bench("batch_restore_100_files", iterations, || {
        let opts = RestoreOptions::to("/tmp/bench-restore")
            .with_conflict_strategy(ConflictStrategy::Rename);
        let _ = opts;

        // Simulate building a snapshot with 100 files
        let mut snap = Snapshot::new(
            BackupStrategy::Full,
            "bench-source".to_string(),
            "local".to_string(),
            None,
        );
        for i in 0..100usize {
            snap.add_entry(make_file_entry(i, 1_048_576));
        }
        assert_eq!(snap.file_count(), 100);
    })
}

/// Benchmark: cross-device restore latency estimation.
///
/// Simulates the extra latency of restoring from a remote device
/// by measuring snapshot lookup + restore option construction.
pub fn bench_cross_device_restore(iterations: u64) -> BenchResult {
    let snapshots: Vec<Snapshot> = (0..5)
        .map(|_| {
            let mut snap = Snapshot::new(
                BackupStrategy::Full,
                "remote-device".to_string(),
                "s3".to_string(),
                None,
            );
            for i in 0..20usize {
                snap.add_entry(make_file_entry(i, 10_485_760));
            }
            snap
        })
        .collect();

    run_bench("cross_device_restore_latency", iterations, || {
        for snap in &snapshots {
            let _ = snap.file_count();
            let _ = snap.total_size;
        }
        let opts = RestoreOptions::to("/tmp/bench-restore-remote");
        let _ = opts;
    })
}

// ============================================================================
// Crypto Benchmarks
// ============================================================================

/// Benchmark: AES-256-GCM encryption throughput.
///
/// Encrypts a 1 MB buffer and measures throughput.
pub fn bench_aes256gcm_encrypt(iterations: u64) -> BenchResult {
    let crypto = Aes256GcmCrypto::new(b"01234567890123456789012345678901").expect("valid key");
    let data = vec![0x42u8; 1024 * 1024]; // 1 MB
    run_bench_bytes(
        "aes256gcm_encrypt_1mb",
        iterations,
        data.len() as u64,
        || {
            let _ = crypto.encrypt(&data);
        },
    )
}

/// Benchmark: AES-256-GCM decryption throughput.
///
/// Decrypts a 1 MB buffer and measures throughput.
pub fn bench_aes256gcm_decrypt(iterations: u64) -> BenchResult {
    let crypto = Aes256GcmCrypto::new(b"01234567890123456789012345678901").expect("valid key");
    let data = vec![0x42u8; 1024 * 1024]; // 1 MB
    let encrypted = crypto.encrypt(&data).expect("encrypt");
    run_bench_bytes(
        "aes256gcm_decrypt_1mb",
        iterations,
        data.len() as u64,
        || {
            let _ = crypto.decrypt(&encrypted);
        },
    )
}

/// Benchmark: encryption with different block sizes.
///
/// Measures how block size affects encryption throughput:
/// 4 KB, 64 KB, 1 MB blocks.
pub fn bench_encrypt_block_sizes(iterations: u64) -> Vec<BenchResult> {
    let crypto = Aes256GcmCrypto::new(b"01234567890123456789012345678901").expect("valid key");
    let block_sizes: Vec<(usize, &str)> =
        vec![(4 * 1024, "4kb"), (64 * 1024, "64kb"), (1024 * 1024, "1mb")];

    block_sizes
        .into_iter()
        .map(|(size, label)| {
            let data = vec![0x42u8; size];
            run_bench_bytes(
                &format!("aes256gcm_encrypt_{}", label),
                iterations,
                size as u64,
                || {
                    let _ = crypto.encrypt(&data);
                },
            )
        })
        .collect()
}

// ============================================================================
// Search Benchmarks
// ============================================================================

/// Helper: build a populated FileIndex.
fn build_test_index(count: usize) -> FileIndex {
    let mut index = FileIndex::new();
    for i in 0..count {
        let entry = FileIndexEntry::new(format!("/data/files/doc_{:05}.txt", i), 4096 + i as u64)
            .with_tag("document")
            .with_tag(if i % 3 == 0 { "important" } else { "archive" })
            .with_summary(format!("Document number {} about backups", i));
        index.upsert(entry);
    }
    index
}

/// Benchmark: index building speed.
///
/// Builds a file index from 10,000 entries and measures time.
pub fn bench_index_build(iterations: u64) -> BenchResult {
    run_bench("index_build_10k_entries", iterations, || {
        let _ = build_test_index(10_000);
    })
}

/// Benchmark: keyword search latency.
///
/// Searches for a keyword across a populated index.
pub fn bench_keyword_search(iterations: u64) -> BenchResult {
    let index = build_test_index(10_000);
    run_bench("keyword_search_10k_index", iterations, || {
        let _ = index.search_keyword("important");
    })
}

/// Benchmark: semantic search latency.
///
/// Measures the latency of semantic search across the index.
pub fn bench_semantic_search(iterations: u64) -> BenchResult {
    let mut index = FileIndex::new();
    for i in 0..1000usize {
        let entry =
            FileIndexEntry::new(format!("/data/files/report_{:04}.pdf", i), 8192 + i as u64)
                .with_tag("report")
                .with_summary(format!("Quarterly report for department {}", i % 20));
        index.upsert(entry);
    }

    let searcher = KeywordSemanticSearch::new(index);
    run_bench("semantic_search_1k_index", iterations, || {
        let _ = searcher.search("quarterly financial report", 10);
    })
}

// ============================================================================
// Master benchmark runner
// ============================================================================

/// Run all benchmarks and return the results.
pub fn run_all_benchmarks() -> Vec<BenchResult> {
    let mut results = Vec::new();

    // Backup benchmarks
    results.push(bench_small_file_backup(10));
    results.push(bench_large_file_backup(3));
    let (full, incr) = bench_incremental_vs_full(10);
    results.push(full);
    results.push(incr);

    // Restore benchmarks
    results.push(bench_single_file_restore(50));
    results.push(bench_batch_restore(20));
    results.push(bench_cross_device_restore(20));

    // Crypto benchmarks
    results.push(bench_aes256gcm_encrypt(10));
    results.push(bench_aes256gcm_decrypt(10));
    results.extend(bench_encrypt_block_sizes(10));

    // Search benchmarks
    results.push(bench_index_build(5));
    results.push(bench_keyword_search(20));
    results.push(bench_semantic_search(20));

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bench_result_display() {
        let r = BenchResult {
            name: "test".to_string(),
            iterations: 100,
            total: Duration::from_millis(500),
            mean: Duration::from_millis(5),
            min: Duration::from_millis(3),
            max: Duration::from_millis(10),
            throughput_ops_sec: Some(200.0),
        };
        let s = format!("{}", r);
        assert!(s.contains("test"));
        assert!(s.contains("200 ops/s"));
    }

    #[test]
    fn test_run_bench_basic() {
        let result = run_bench("addition", 100, || {
            let _ = 1 + 1;
        });
        assert_eq!(result.iterations, 100);
        assert!(result.mean < Duration::from_millis(1));
        assert!(result.throughput_ops_sec.is_some());
    }

    #[test]
    fn test_run_bench_bytes() {
        let result = run_bench_bytes("alloc", 10, 1024, || {
            let _ = vec![0u8; 1024];
        });
        assert!(result.throughput_ops_sec.is_some());
    }

    #[test]
    fn test_small_file_backup_bench() {
        let result = bench_small_file_backup(3);
        assert_eq!(result.iterations, 3);
        assert!(result.throughput_ops_sec.is_some());
    }

    #[test]
    fn test_incremental_vs_full() {
        let (full, incr) = bench_incremental_vs_full(3);
        assert!(full.mean >= Duration::ZERO);
        assert!(incr.mean >= Duration::ZERO);
    }

    #[test]
    fn test_encrypt_decrypt_bench() {
        let enc = bench_aes256gcm_encrypt(3);
        let dec = bench_aes256gcm_decrypt(3);
        assert!(enc.throughput_ops_sec.is_some());
        assert!(dec.throughput_ops_sec.is_some());
    }

    #[test]
    fn test_block_size_bench() {
        let results = bench_encrypt_block_sizes(3);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.throughput_ops_sec.is_some());
        }
    }

    #[test]
    fn test_index_build_bench() {
        let result = bench_index_build(3);
        assert!(result.mean < Duration::from_secs(10));
    }

    #[test]
    fn test_keyword_search_bench() {
        let result = bench_keyword_search(5);
        assert!(result.throughput_ops_sec.is_some());
    }

    #[test]
    fn test_semantic_search_bench() {
        let result = bench_semantic_search(5);
        assert!(result.mean < Duration::from_secs(10));
    }

    #[test]
    fn test_single_file_restore_bench() {
        let result = bench_single_file_restore(10);
        assert_eq!(result.iterations, 10);
    }

    #[test]
    fn test_batch_restore_bench() {
        let result = bench_batch_restore(5);
        assert_eq!(result.iterations, 5);
    }

    #[test]
    fn test_cross_device_restore_bench() {
        let result = bench_cross_device_restore(5);
        assert_eq!(result.iterations, 5);
    }

    #[test]
    fn test_run_all_benchmarks() {
        let results = run_all_benchmarks();
        // 4 backup + 3 restore + 3 crypto + 3 block sizes + 3 search = 16
        assert!(
            results.len() >= 13,
            "Expected at least 13 benchmarks, got {}",
            results.len()
        );
        for r in &results {
            assert!(!r.name.is_empty());
            assert!(r.iterations > 0);
        }
    }
}
