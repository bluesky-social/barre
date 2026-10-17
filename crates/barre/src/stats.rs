//! barre-only, bench/debug instrumentation (cargo feature `stats`): counts
//! `calculate_layout_internal` calls per node per layout, by pass type and
//! cache outcome, and prints one summary line per layout to stderr when the
//! `BARRE_STATS` environment variable is set. Without the feature, the
//! [`stat!`](crate::stat) macro expands to nothing, so release builds carry
//! no trace of it. See docs/BENCH.md ("Layout calls per node").

/// Runs `$f(&mut Stats)` when the `stats` feature is on; nothing otherwise.
#[macro_export]
macro_rules! stat {
    ($f:expr) => {
        #[cfg(feature = "stats")]
        $crate::stats::with($f);
    };
}

#[cfg(feature = "stats")]
pub use imp::*;

#[cfg(feature = "stats")]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// How a `calculate_layout_internal` call ended.
    #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
    pub enum Outcome {
        /// Answered by an entry stored earlier in this layout.
        HitSameGen,
        /// Answered by an entry stored by an earlier layout.
        HitEarlierGen,
        /// No usable entry: ran `calculate_layout_impl`.
        Miss,
        /// Dirty / config / direction change invalidated the cache: ran.
        Visit,
        /// An earlier layout's entry refused by T-0006's D5: ran.
        Rejected,
    }

    #[derive(Default, Debug)]
    pub struct NodeStat {
        pub calls: u32,
        pub runs: u32,
        pub measure_runs: u32,
        pub layout_runs: u32,
        pub replays: u32,
        pub has_measure_func: bool,
    }

    #[derive(Default)]
    pub struct Stats {
        pub layouts: u64,
        /// (perform_layout, reason, outcome) -> count.
        pub calls: HashMap<(bool, u8, Outcome), u32>,
        pub nodes: HashMap<usize, NodeStat>,
        pub impl_calls: u32,
        pub replays: u32,
        pub replays_retainable: u32,
        pub replays_direct_retainable: u32,
        pub measure_callbacks: u32,
        /// Runs whose result equalled a same-generation entry with different
        /// available sizes (a candidate for being provably redundant).
        pub run_equal_to_existing: u32,
        /// Runs with perform_layout=false whose (w, h) result equals an
        /// existing measurement entry for the same node (any generation).
        pub measure_run_result_seen: u32,
    }

    thread_local! {
        static STATS: RefCell<Stats> = RefCell::new(Stats::default());
    }

    pub fn with(f: impl FnOnce(&mut Stats)) {
        STATS.with(|s| f(&mut s.borrow_mut()));
    }

    pub fn node_id<T>(layout: &T) -> usize {
        layout as *const T as usize
    }

    const REASONS: [&str; 9] = [
        "Initial",
        "AbsLayout",
        "Stretch",
        "MultilineStretch",
        "FlexLayout",
        "MeasureChild",
        "AbsMeasureChild",
        "FlexMeasure",
        "GridLayout",
    ];

    /// Called at the start of `calculate_layout`.
    pub fn begin_layout() {
        with(|s| {
            s.calls.clear();
            s.nodes.clear();
            s.impl_calls = 0;
            s.replays = 0;
            s.replays_retainable = 0;
            s.replays_direct_retainable = 0;
            s.measure_callbacks = 0;
            s.run_equal_to_existing = 0;
            s.measure_run_result_seen = 0;
        });
    }

    /// Called at the end of `calculate_layout`: prints a summary line when
    /// `BARRE_STATS` is set.
    pub fn end_layout() {
        if std::env::var_os("BARRE_STATS").is_none() {
            return;
        }
        with(|s| {
            s.layouts += 1;
            let total: u32 = s.calls.values().sum();
            let n = s.nodes.len();
            let mut hist: std::collections::BTreeMap<u32, u32> = Default::default();
            let mut run_hist: std::collections::BTreeMap<u32, u32> = Default::default();
            let (mut leaf_runs, mut container_runs) = (0, 0);
            for ns in s.nodes.values() {
                *hist.entry(ns.calls).or_default() += 1;
                *run_hist.entry(ns.runs).or_default() += 1;
                if ns.has_measure_func {
                    leaf_runs += ns.runs;
                } else {
                    container_runs += ns.runs;
                }
            }
            let mut out = format!(
                "STATS layout={} nodes={} internal_calls={} ({:.2}/node) impl_calls={} replays={} (subtree-retainable {}, direct {}) measure_cb={} runs: leaf={} container={} run_eq_same_gen={} measure_run_seen={}\n",
                s.layouts,
                n,
                total,
                total as f64 / n.max(1) as f64,
                s.impl_calls,
                s.replays,
                s.replays_retainable,
                s.replays_direct_retainable,
                s.measure_callbacks,
                leaf_runs,
                container_runs,
                s.run_equal_to_existing,
                s.measure_run_result_seen,
            );
            let mut keys: Vec<_> = s.calls.iter().collect();
            keys.sort_by_key(|(k, _)| (k.0, k.1, format!("{:?}", k.2)));
            for ((perform, reason, outcome), count) in keys {
                out += &format!(
                    "  {:<6} {:<17} {:<14} {}\n",
                    if *perform { "layout" } else { "measure" },
                    REASONS[*reason as usize],
                    format!("{outcome:?}"),
                    count
                );
            }
            out += &format!("  calls/node histogram: {hist:?}\n");
            out += &format!("  runs/node histogram: {run_hist:?}\n");
            eprint!("{out}");
        });
    }
}
