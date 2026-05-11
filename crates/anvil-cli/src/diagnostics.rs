//! Render `anvil-core` [`Diagnostic`] values as `miette::Report`s.
//!
//! `anvil-core` deliberately stays I/O-free, so it ships only structured
//! diagnostic data: a kind, a primary span, and zero or more related
//! spans, each carrying a file path and byte offsets. This module is the
//! one place that converts those into terminal-rendered, source-snippet
//! diagnostics by reading the file contents and wiring up `miette`.
//!
//! Multi-file diagnostics: `miette` attaches one `NamedSource` per
//! `Report`, so labels in files other than the primary one are emitted as
//! human-readable note lines after the snippet rather than inline
//! highlights. This matches what `rustc` does for cross-file errors.

use std::fs;

use anvil_core::ir::SourceSpan;
use anvil_core::validate::{Diagnostic, Label};
use miette::{LabeledSpan, MietteDiagnostic, NamedSource, Report, Severity};

/// Convert a structured [`Diagnostic`] into a printable `miette::Report`.
#[must_use]
pub fn render(d: &Diagnostic) -> Report {
    // Primary file becomes the report's source code; its labels render
    // inline. Labels in other files become trailing help-text notes.
    let primary_path = d.primary.span.path.clone();
    let primary_src = read_or_blank(&primary_path);

    let mut labels: Vec<LabeledSpan> = Vec::new();
    let mut other_file_notes: Vec<String> = Vec::new();

    push_label(
        &mut labels,
        &mut other_file_notes,
        &primary_path,
        &d.primary,
    );
    for r in &d.related {
        push_label(&mut labels, &mut other_file_notes, &primary_path, r);
    }

    let help = if other_file_notes.is_empty() {
        None
    } else {
        Some(other_file_notes.join("\n"))
    };

    let mut diag = MietteDiagnostic::new(d.summary())
        .with_code(d.code())
        .with_severity(Severity::Error)
        .with_labels(labels);
    if let Some(h) = help {
        diag = diag.with_help(h);
    }

    Report::new(diag).with_source_code(NamedSource::new(primary_path, primary_src))
}

/// Read `path` to a `String`, returning an empty string on any error so
/// rendering still succeeds (with no source snippet) when the file has
/// been deleted between parse and render.
fn read_or_blank(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn push_label(
    labels: &mut Vec<LabeledSpan>,
    other_file_notes: &mut Vec<String>,
    primary_path: &str,
    label: &Label,
) {
    if label.span.path == primary_path {
        labels.push(span_to_labeled(&label.span, Some(label.message.clone())));
    } else {
        other_file_notes.push(format!(
            "{} (at {}:{}..{})",
            label.message, label.span.path, label.span.start, label.span.end
        ));
    }
}

fn span_to_labeled(s: &SourceSpan, msg: Option<String>) -> LabeledSpan {
    LabeledSpan::new(msg, s.start as usize, s.len() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::ir::{Key, ModulePath};
    use anvil_core::validate::DiagnosticKind;

    fn k(name: &str) -> Key {
        Key::class(ModulePath::from_abs(format!("/p/{name}.ts")), name)
    }

    #[test]
    fn render_does_not_panic_on_missing_file() {
        let d = Diagnostic {
            kind: DiagnosticKind::MissingBinding {
                key: k("Heater"),
                requested_by: None,
            },
            primary: Label {
                span: SourceSpan::new("/does/not/exist.ts", 0, 5),
                message: "missing".into(),
            },
            related: vec![],
        };
        // The Debug impl of Report is what we print; just exercise it.
        let r = render(&d);
        let _ = format!("{r:?}");
    }
}
