//! ENF-05: scan of 500 000 fake files in under 60 s.
//!
//! The tree lives in memory (`FakeFileSystem`): this measures the engine
//! (listing, matching, scoring, walking), not the disk.

// Benchmark code is test code: `expect` is allowed here as in tests.
#![allow(clippy::expect_used)]

use std::sync::atomic::AtomicBool;

use chrono::{Duration, Utc};
use cleanslate_core::fs::FakeFileSystem;
use cleanslate_core::model::ScanOptions;
use cleanslate_core::registry::WinRegistry;
use cleanslate_core::safety::{KnownFolders, SafetyRules};
use cleanslate_core::scan::{scan_orphans, ScanEnv};
use criterion::{criterion_group, criterion_main, Criterion};

const FOLDERS: usize = 5_000;
const FILES_PER_FOLDER: usize = 100;

fn tree() -> FakeFileSystem {
    let old = Some(Utc::now() - Duration::days(200));
    let mut fs = FakeFileSystem::new();
    for folder in 0..FOLDERS {
        let root = if folder % 2 == 0 {
            r"C:\Users\Ana\AppData\Roaming"
        } else {
            r"C:\Users\Ana\AppData\Local"
        };
        for file in 0..FILES_PER_FOLDER {
            fs.add_file(
                format!(r"{root}\Vendor{folder}\sub{}\file{file}.dat", file % 10),
                1_024,
                old,
            );
        }
    }
    fs
}

fn bench(c: &mut Criterion) {
    let fs = tree();
    let folders = KnownFolders::under(r"C:\");
    let rules = SafetyRules::new(&folders, &[]);
    // Read-only, never created: the registry part finds nothing.
    let registry = WinRegistry::sandboxed(r"Software\CleanSlateTest\bench-never-created")
        .expect("valid test prefix");
    let cancel = AtomicBool::new(false);
    let env = ScanEnv {
        fs: &fs,
        registry: &registry,
        folders: &folders,
        rules: &rules,
        apps: &[],
        extra_executables: &[],
        cancel: &cancel,
        now: Utc::now(),
        installed_packages: None,
    };
    let mut group = c.benchmark_group("enf05");
    group.sample_size(10);
    group.bench_function("scan_500k_files", |b| {
        b.iter(|| {
            let report = scan_orphans(&env, &ScanOptions::default(), &|_| {});
            assert_eq!(report.groups.len(), FOLDERS);
            report
        })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
