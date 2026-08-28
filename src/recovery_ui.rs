use std::io;
use std::path::Path;

use crate::recovery::{MaintenancePlan, retained_time_label};
use crate::terminal::{Interaction, byte_count};

pub fn render_maintenance(
    interaction: &mut impl Interaction,
    report: &MaintenancePlan,
    max_bytes: u64,
    always_show: bool,
    library_root: Option<&Path>,
) -> io::Result<()> {
    let deferred = report.usage_after > max_bytes;
    if !always_show && report.evictions.is_empty() && !deferred {
        return Ok(());
    }

    interaction.section_heading("Recovery maintenance")?;
    if let Some(library_root) = library_root {
        interaction.path_field("Library", library_root.display().to_string())?;
    }
    for eviction in &report.evictions {
        let retained_at = retained_time_label(eviction.retained_at)
            .unwrap_or_else(|_| "unknown retention time".into());
        interaction.success(format!(
            "✓ Removed {} · {retained_at} · freed {}.",
            eviction.display_label,
            byte_count(eviction.size_bytes)
        ))?;
    }
    if report.evictions.is_empty() && !deferred {
        interaction.prose("  No eligible retained copies needed eviction.")?;
    }
    interaction.field(
        "Recovery usage",
        format!(
            "{} / {}",
            byte_count(report.usage_after),
            byte_count(max_bytes)
        ),
    )?;
    if deferred {
        interaction.warning(
            "Cleanup deferred: protected retained copies keep recovery storage over its limit.",
        )?;
        if let Some(deadline) = report.earliest_protected_until {
            interaction.field(
                "Earliest cleanup eligibility",
                retained_time_label(deadline)
                    .unwrap_or_else(|_| "unknown protection deadline".into()),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::recovery::MaintenancePlan;
    use crate::terminal::UiLine;

    #[test]
    fn protected_excess_is_reported_as_deferred_with_earliest_eligibility() {
        let report = MaintenancePlan {
            usage_before: 200,
            usage_after: 200,
            evictions: Vec::new(),
            earliest_protected_until: Some(2_000_000_000),
        };
        let mut interaction = RecordingInteraction::default();

        render_maintenance(&mut interaction, &report, 100, false, None).unwrap();

        assert!(interaction.output.contains("Recovery maintenance"));
        assert!(interaction.output.contains("Cleanup deferred"));
        assert!(interaction.output.contains("Earliest cleanup eligibility"));
        assert!(interaction.output.contains("2033-05-18 03:33:20 UTC"));
    }

    #[derive(Default)]
    struct RecordingInteraction {
        output: String,
    }

    impl Interaction for RecordingInteraction {
        fn present(&mut self, line: UiLine) -> io::Result<()> {
            self.output.push_str(&line.plain_text());
            self.output.push('\n');
            Ok(())
        }

        fn prompt(&mut self, _prompt: UiLine) -> io::Result<String> {
            unreachable!("maintenance rendering is non-interactive")
        }
    }
}
