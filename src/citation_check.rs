//! The Citation Check report: an append-only Markdown table that fills in as
//! background checks finish.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ragrig_gpui::{CitationCheckRow, CitationCheckSummary};

/// Report file in the workspace root.
pub const REPORT_FILE: &str = "Citation_check.md";

/// One citation-check run, rendered into a new section of the report.
#[derive(Clone, Debug)]
pub struct CitationCheckRun {
    /// Absolute path of the report file.
    pub path: PathBuf,
    /// Content already in the file when the run started.
    prefix: String,
    /// Document that supplied the selection.
    pub document: String,
    /// Character range of the selection.
    pub selection: Range<usize>,
    /// Citation occurrences found.
    pub total: usize,
    /// Rows by occurrence index.
    pub rows: BTreeMap<usize, CitationCheckRow>,
    /// Chat model label.
    pub model: String,
    /// When the run started.
    pub started: SystemTime,
    /// Whether the worker finished (successfully or with a job-level error).
    pub finished: bool,
    /// Whether the user stopped the run.
    pub stopped: bool,
    /// Job-level error.
    pub error: Option<String>,
    /// Totals reported on completion.
    pub summary: Option<CitationCheckSummary>,
}

impl CitationCheckRun {
    /// Start a run and capture the existing report so new results append.
    pub fn begin(
        path: PathBuf,
        document: String,
        selection: Range<usize>,
        model: String,
    ) -> Result<Self, String> {
        let existing = match fs::read_to_string(&path) {
            Ok(existing) => existing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(format!("cannot read '{}': {error}", path.display()));
            }
        };
        let prefix = if existing.trim().is_empty() {
            "# Citation check\n".to_owned()
        } else {
            let mut prefix = existing.trim_end().to_owned();
            prefix.push_str("\n\n");
            prefix
        };
        Ok(Self {
            path,
            prefix,
            document,
            selection,
            total: 0,
            rows: BTreeMap::new(),
            model,
            started: SystemTime::now(),
            finished: false,
            stopped: false,
            error: None,
            summary: None,
        })
    }

    /// Record the total found by the worker.
    pub fn started(&mut self, total: usize) {
        self.total = total;
    }

    /// Add or replace one row.
    pub fn push(&mut self, row: CitationCheckRow) {
        self.rows.insert(row.index, row);
    }

    /// Mark the run finished with its totals.
    pub fn finish(&mut self, summary: CitationCheckSummary) {
        self.summary = Some(summary);
        self.finished = true;
    }

    /// Mark the run stopped by the user.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.finished = true;
    }

    /// Mark a job-level failure.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.finished = true;
    }

    /// Markdown for the whole file, including earlier sections.
    pub fn render(&self) -> String {
        let mut out = self.prefix.clone();
        out.push_str(&format!(
            "## Citation check — {}\n\n",
            format_timestamp(self.started)
        ));
        out.push_str(&format!(
            "Document: `{}` · selection chars {}–{}\n\n",
            self.document, self.selection.start, self.selection.end
        ));
        out.push_str(&format!("Model: {}\n\n", escape_cell(&self.model)));

        if let Some(error) = &self.error {
            out.push_str(&format!("**Check failed:** {}\n\n", escape_cell(error)));
        }
        if self.total == 0 && self.finished {
            out.push_str("No `@key` citations found in the selection.\n\n");
        }

        if !self.rows.is_empty() {
            out.push_str(
                "| # | Citation | Claim | Verdict | Evidence | Quote check | Source | Error |\n\
                 |---:|---|---|---|---|---|---|---|\n",
            );
            for row in self.rows.values() {
                out.push_str(&self.row_markdown(row));
            }
            out.push('\n');
        } else if !self.finished {
            out.push_str("_Checking citations…_\n\n");
        }

        if let Some(summary) = self.summary {
            out.push_str(&format!(
                "**Result:** {} checked · {} accurate · {} approximate · {} not found · {} errors.\n\n",
                summary.checked,
                summary.accurate,
                summary.approximate,
                summary.not_found,
                summary.errors
            ));
        }
        if self.stopped {
            out.push_str(&format!(
                "**Stopped after {} of {} citations.**\n\n",
                self.rows.len(),
                self.total
            ));
        }
        out
    }

    fn row_markdown(&self, row: &CitationCheckRow) -> String {
        let verdict = match (row.verdict, row.verdict_label.as_deref()) {
            (Some(number), Some(label)) if !row.explanation.trim().is_empty() => format!(
                "{number} — {label}<br>{}",
                escape_cell(&truncate(&row.explanation, 400))
            ),
            (Some(number), Some(label)) => format!("{number} — {label}"),
            _ => "—".to_owned(),
        };
        let evidence = if row.quotes.is_empty() {
            "—".to_owned()
        } else {
            row.quotes
                .iter()
                .map(|quote| format!("“{}”", escape_cell(&quote.text)))
                .collect::<Vec<_>>()
                .join("<br>")
        };
        let quote_check = if row.quotes.is_empty() {
            "—".to_owned()
        } else {
            row.quotes
                .iter()
                .map(|quote| quote.quality.label())
                .collect::<Vec<_>>()
                .join("<br>")
        };
        let error = row.error.as_deref().unwrap_or("—");
        format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            row.index,
            escape_cell(&row.citation),
            escape_cell(&truncate(&row.claim, 240)),
            verdict,
            evidence,
            quote_check,
            escape_cell(&row.source),
            escape_cell(error)
        )
    }
}

/// Report path inside `workspace`.
pub fn report_path(workspace: &Path) -> PathBuf {
    workspace.join(REPORT_FILE)
}

/// Write the report atomically so an interrupted run cannot truncate it.
pub fn write_report(path: &Path, content: &str) -> Result<(), String> {
    let temporary = path.with_extension("md.tmp");
    fs::write(&temporary, content)
        .map_err(|error| format!("cannot write '{}': {error}", temporary.display()))?;
    if fs::rename(&temporary, path).is_ok() {
        return Ok(());
    }
    // Windows cannot replace an existing file with `rename`; fall back to a
    // direct write and remove the temporary file.
    let result = fs::write(path, content)
        .map_err(|error| format!("cannot write '{}': {error}", path.display()));
    let _ = fs::remove_file(&temporary);
    result
}

/// Escape one Markdown table cell.
pub fn escape_cell(text: &str) -> String {
    text.replace('\r', " ")
        .replace('\n', "<br>")
        .replace('|', "\\|")
        .trim()
        .to_owned()
}

fn truncate(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('…');
    }
    out
}

/// UTC timestamp without a date-time dependency.
pub fn format_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragrig_gpui::{CitationQuote, QuoteQuality};

    fn row(index: usize, citation: &str, verdict: Option<u8>) -> CitationCheckRow {
        CitationCheckRow {
            index,
            citation: citation.to_owned(),
            claim: "The treatment reduces response times.".to_owned(),
            verdict,
            verdict_label: verdict.map(|verdict| {
                match verdict {
                    5 => "empirically justified",
                    _ => "indirectly",
                }
                .to_owned()
            }),
            explanation: "Because the experiment measured it.".to_owned(),
            quotes: vec![
                CitationQuote {
                    text: "response times fell".to_owned(),
                    quality: QuoteQuality::Accurate,
                },
                CitationQuote {
                    text: "the experiment measured response times".to_owned(),
                    quality: QuoteQuality::Approximate,
                },
            ],
            source: "Smith (2020) Title".to_owned(),
            error: None,
        }
    }

    #[test]
    fn a_run_appends_a_new_section() {
        let path = temp_dir().join(REPORT_FILE);
        fs::write(&path, "# Citation check\n\nEarlier run.\n").expect("seed");
        let mut run = CitationCheckRun::begin(
            path.clone(),
            "draft.md".to_owned(),
            10..20,
            "Ollama / test".to_owned(),
        )
        .expect("run");
        run.started(1);
        run.push(row(1, "@Smith2020", Some(5)));
        run.finish(CitationCheckSummary {
            total: 1,
            checked: 1,
            errors: 0,
            accurate: 1,
            approximate: 1,
            not_found: 0,
        });
        let text = run.render();
        assert!(text.starts_with("# Citation check\n\nEarlier run.\n\n## Citation check"));
        assert!(text.contains("| 1 | @Smith2020 |"));
        assert!(text.contains("5 — empirically justified"));
        assert!(text.contains("**Result:** 1 checked"));
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn cells_escape_pipes_and_newlines() {
        assert_eq!(escape_cell("a | b\nc"), "a \\| b<br>c");
    }

    #[test]
    fn quote_matches_reach_their_own_column() {
        let path = temp_dir().join(REPORT_FILE);
        let mut run =
            CitationCheckRun::begin(path, "draft.md".to_owned(), 0..10, "model".to_owned())
                .expect("run");
        run.started(1);
        run.push(row(1, "@Smith2020", Some(5)));
        let text = run.render();
        assert!(text.contains("accurate<br>approximate"));
    }

    #[test]
    fn a_failed_check_renders_the_error_column() {
        let path = temp_dir().join(REPORT_FILE);
        let mut run =
            CitationCheckRun::begin(path, "draft.md".to_owned(), 0..10, "model".to_owned())
                .expect("run");
        run.started(1);
        let mut failed = row(1, "@Smith2020", None);
        failed.error = Some("full PDF exceeded the source budget; used 2 excerpt(s)".to_owned());
        run.push(failed);
        let text = run.render();
        assert!(text.contains("excerpt"));
    }

    #[test]
    fn an_empty_selection_reports_no_citations() {
        let path = temp_dir().join(REPORT_FILE);
        let mut run =
            CitationCheckRun::begin(path, "draft.md".to_owned(), 0..10, "model".to_owned())
                .expect("run");
        run.started(0);
        run.finish(CitationCheckSummary::default());
        assert!(run.render().contains("No `@key` citations"));
    }

    #[test]
    fn timestamps_are_utc() {
        assert_eq!(format_timestamp(UNIX_EPOCH), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            format_timestamp(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
            "2023-11-14 22:13:20 UTC"
        );
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scico-citation-check-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
