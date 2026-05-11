//! Implementation of `anvil watch`.
//!
//! After an initial build, watches the configured root directory and
//! re-emits any component whose dependency closure intersects a
//! changed file. The "closure" is the set of source files the parser
//! traversed when building that component's graph.
//!
//! Filesystem events are debounced (default 100ms) to coalesce the
//! flurry of writes editors emit on save.
//!
//! The watch loop runs forever; the user terminates with Ctrl+C
//! (`SIGINT`/`CTRL_C_EVENT`). Tests can short-circuit the loop by
//! setting the `ANVIL_WATCH_ITERATIONS` env var to a positive integer.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anvil_codegen::emit_component;
use anvil_core::graph::{build_and_validate, GraphInput};
use anvil_core::validate::Diagnostic;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::diagnostics;
use crate::{load_project, ProjectIr};

/// Inputs needed to start a watch session.
pub(crate) struct Plan {
    /// Entry `.ts` files to keep regenerated. Each is treated as an
    /// independent project.
    pub entries: Vec<PathBuf>,
    /// Optional `tsconfig.json` for the resolver.
    pub tsconfig: Option<PathBuf>,
    /// Optional plugins
    pub plugins: Vec<String>,
    /// Directory the watcher recursively observes.
    pub watch_root: PathBuf,
}

/// How long to coalesce contiguous filesystem events before reacting.
/// 100ms is enough to swallow typical editor save bursts on macOS,
/// Linux, and Windows without feeling laggy.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// Map of entry → its current dependency closure (set of absolute
/// source-file paths).
type ClosureMap = HashMap<PathBuf, HashSet<PathBuf>>;

/// Run the watch loop until terminated.
pub(crate) fn run(plan: &Plan) -> notify::Result<()> {
    let mut closures: ClosureMap = HashMap::new();

    eprintln!("anvil: watching {}", plan.watch_root.display());
    // Initial pass: build everything and learn the closures.
    for entry in &plan.entries {
        match rebuild_one(entry, plan.tsconfig.clone(), &plan.plugins) {
            Ok(closure) => {
                closures.insert(entry.clone(), closure);
            }
            Err(msg) => {
                eprintln!("anvil: initial build of {} failed: {msg}", entry.display());
                // Still register the entry with an empty closure so subsequent
                // edits to *that* file at least re-trigger a rebuild attempt.
                let mut singleton = HashSet::new();
                singleton.insert(entry.clone());
                closures.insert(entry.clone(), singleton);
            }
        }
    }

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(&plan.watch_root, RecursiveMode::Recursive)?;

    let max_iters = std::env::var("ANVIL_WATCH_ITERATIONS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok());
    let mut iter = 0u32;

    loop {
        if let Some(max) = max_iters {
            if iter >= max {
                eprintln!("anvil: ANVIL_WATCH_ITERATIONS={max} reached; exiting");
                return Ok(());
            }
        }

        // Block until something changes.
        let Ok(first) = rx.recv() else {
            return Ok(()); // sender dropped → exit cleanly
        };
        let mut changed = HashSet::<PathBuf>::new();
        absorb_event(first, &mut changed);

        // Coalesce a burst of follow-up events.
        let deadline = Instant::now() + DEBOUNCE;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_timeout(deadline - now) {
                Ok(ev) => absorb_event(ev, &mut changed),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        if changed.is_empty() {
            continue;
        }

        // Decide which entries to rebuild.
        let to_rebuild: Vec<PathBuf> = closures
            .iter()
            .filter_map(|(entry, closure)| {
                if changed.iter().any(|c| closure.contains(c)) {
                    Some(entry.clone())
                } else {
                    None
                }
            })
            .collect();

        if to_rebuild.is_empty() {
            continue;
        }

        for entry in &to_rebuild {
            match rebuild_one(entry, plan.tsconfig.clone(), &plan.plugins) {
                Ok(new_closure) => {
                    closures.insert(entry.clone(), new_closure);
                }
                Err(msg) => {
                    eprintln!("anvil: rebuild of {} failed: {msg}", entry.display());
                }
            }
        }

        iter = iter.saturating_add(1);
    }
}

/// Drain a `notify::Event` into `out`, restricting to paths that look
/// like TypeScript source files. We ignore directory creation events
/// and bookkeeping types — anything else flows through and may
/// optimistically trigger a rebuild.
fn absorb_event(ev: notify::Result<notify::Event>, out: &mut HashSet<PathBuf>) {
    let Ok(ev) = ev else {
        return;
    };
    match ev.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return,
    }
    for p in ev.paths {
        // Watch events surface OS-native paths; canonicalize when the file
        // still exists so they compare equal to closure entries.
        let canon = std::fs::canonicalize(&p).unwrap_or(p);
        if is_source_ts(&canon) {
            out.insert(canon);
        }
    }
}

fn is_source_ts(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "json")
    )
}

/// Re-run validate + emit for a single entry. Returns the entry's
/// (recomputed) source-file closure on success.
fn rebuild_one(
    entry: &Path,
    tsconfig: Option<PathBuf>,
    plugins: &[String],
) -> Result<HashSet<PathBuf>, String> {
    let started = Instant::now();
    let ir = load_project(entry, tsconfig, plugins).map_err(stringify_check_error)?;

    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    for c in &ir.components {
        let (_g, ds) = build_and_validate(GraphInput {
            component: c,
            modules: &ir.modules,
            inject_classes: &ir.inject_classes,
            subcomponents: &ir.subcomponents,
        });
        all_diagnostics.extend(ds);
    }
    if !all_diagnostics.is_empty() {
        for d in &all_diagnostics {
            eprintln!("{:?}", diagnostics::render(d));
        }
        return Err(format!(
            "{} validation diagnostic(s)",
            all_diagnostics.len()
        ));
    }

    let version = env!("CARGO_PKG_VERSION");
    let mut written = 0usize;
    for c in &ir.components {
        let code = emit_component(
            c,
            &ir.modules,
            &ir.inject_classes,
            &ir.subcomponents,
            version,
        )
        .map_err(|e| format!("emit failed: {e}"))?;
        let out = output_path_for(&c.class.module.abs)
            .ok_or_else(|| format!("bad component path: {}", c.class.module.abs))?;
        std::fs::write(&out, &code).map_err(|e| format!("write {}: {e}", out.display()))?;
        written += 1;
    }
    let elapsed = started.elapsed();
    eprintln!(
        "anvil: rebuilt {} ({} component(s)) in {}ms",
        entry.display(),
        written,
        elapsed.as_millis(),
    );
    Ok(closure_of(&ir))
}

fn closure_of(ir: &ProjectIr) -> HashSet<PathBuf> {
    ir.files
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect()
}

fn output_path_for(component_module: &str) -> Option<PathBuf> {
    let p = PathBuf::from(component_module);
    let parent = p.parent()?;
    let stem = p.file_stem()?;
    let mut out = parent.join(stem);
    out.as_mut_os_string().push(".anvil.ts");
    Some(out)
}

fn stringify_check_error(e: crate::CheckError) -> String {
    match e {
        crate::CheckError::Diagnostics(n) => format!("{n} validation diagnostic(s)"),
        crate::CheckError::Other(err) => format!("{err:#}"),
    }
}
