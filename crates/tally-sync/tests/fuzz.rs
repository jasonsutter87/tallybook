//! Random operations on two or three devices, with random sync points and
//! now and then a lost request or response, must always end with identical,
//! balanced books. Each seed is a different history; a failure names its seed
//! and the run can be repeated with `FUZZ_SEED=<n> cargo test -p tally-sync
//! --test fuzz one_seed -- --nocapture`.

mod common;

use common::*;
use tally_sync::Fault;

struct Stats {
    seeds: u64,
    operations: u64,
    syncs: u64,
    faults: u64,
    renames: usize,
    merged_voids: usize,
    set_aside: usize,
}

/// Runs one random history and checks the devices agree at the end.
fn run_seed(seed: u64, stats: &mut Stats) {
    let mut rng = Rng::new(seed);
    let devices = 2 + rng.below(2);
    let mut world = World::new(devices);
    let steps = 25 + rng.below(40);
    for step in 0..steps {
        let who = rng.below(devices);
        // Some bursts of work on one device while the others are offline.
        for _ in 0..1 + rng.below(4) {
            if random_op(&mut world.devs[who].store, &mut rng) {
                stats.operations += 1;
            }
        }
        if step % 3 == 0 {
            tick();
        }
        if rng.chance(35) {
            let who = rng.below(devices);
            if rng.chance(15) {
                let fault = if rng.chance(50) { Fault::RequestLost } else { Fault::ResponseLost };
                world.devs[who].net.fail(rng.below(4), fault);
                stats.faults += 1;
            }
            // A lost request or response is an error; a retry later converges.
            if let Ok(report) = world.devs[who].sync(&world.key) {
                stats.syncs += 1;
                stats.renames += report.applied.renamed.len();
                stats.merged_voids += report.applied.voids_merged;
                stats.set_aside += report.applied.set_aside.len();
            }
        }
    }
    // Everyone catches up. Injected faults fire once, so a retry is clean.
    for dev in &mut world.devs {
        let _ = dev.sync(&world.key);
    }
    for report in world.sync_everyone() {
        stats.syncs += 1;
        stats.renames += report.applied.renamed.len();
        stats.merged_voids += report.applied.voids_merged;
        stats.set_aside += report.applied.set_aside.len();
    }
    for (i, dev) in world.devs.iter().enumerate() {
        assert_eq!(dev.store.sync_outbox_len().unwrap(), 0, "seed {seed}: device {i} has unsent changes");
        assert_eq!(dev.store.sync_uncovered_columns().unwrap(), Vec::<String>::new());
    }
    let stores: Vec<&tally_store::Store> = world.devs.iter().map(|d| &d.store).collect();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_same_books(&stores)));
    if let Err(e) = result {
        eprintln!("seed {seed} diverged");
        std::panic::resume_unwind(e);
    }
    stats.seeds += 1;
    world.finish();
}

#[test]
fn random_histories_on_two_and_three_devices_converge() {
    let seeds: u64 = std::env::var("FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut stats = Stats { seeds: 0, operations: 0, syncs: 0, faults: 0, renames: 0, merged_voids: 0, set_aside: 0 };
    for seed in 0..seeds {
        run_seed(seed, &mut stats);
    }
    eprintln!(
        "fuzz: {} seeds, {} operations, {} syncs, {} injected faults; {} renames, {} merged voids, {} dropped clashes",
        stats.seeds, stats.operations, stats.syncs, stats.faults, stats.renames, stats.merged_voids, stats.set_aside
    );
    assert!(stats.renames > 0 && stats.merged_voids > 0, "the histories should actually produce conflicts");
}

/// The heavy variant: many more seeds. `cargo test --release -p tally-sync
/// --test fuzz -- --ignored` (the CI performance job runs it).
#[test]
#[ignore]
fn many_more_random_histories_converge() {
    let mut stats = Stats { seeds: 0, operations: 0, syncs: 0, faults: 0, renames: 0, merged_voids: 0, set_aside: 0 };
    for seed in 1_000..1_600 {
        run_seed(seed, &mut stats);
    }
    eprintln!(
        "heavy fuzz: {} seeds, {} operations, {} syncs, {} injected faults; {} renames, {} merged voids, {} dropped clashes",
        stats.seeds, stats.operations, stats.syncs, stats.faults, stats.renames, stats.merged_voids, stats.set_aside
    );
}

#[test]
#[ignore]
fn one_seed() {
    let seed: u64 = std::env::var("FUZZ_SEED").expect("set FUZZ_SEED").parse().unwrap();
    let mut stats = Stats { seeds: 0, operations: 0, syncs: 0, faults: 0, renames: 0, merged_voids: 0, set_aside: 0 };
    run_seed(seed, &mut stats);
}
