use domain::{ExtractionError, SessionTranscript};

pub(crate) fn validate_transcript_limits(
    transcript: &SessionTranscript,
    max_entries: usize,
    max_entry_chars: usize,
    max_total_chars: usize,
) -> Result<(), ExtractionError> {
    if transcript.entries.len() > max_entries {
        return Err(ExtractionError::InvalidTranscript(format!(
            "transcript entry count {} exceeds maximum {}",
            transcript.entries.len(),
            max_entries
        )));
    }

    let mut total_chars = 0usize;
    for (index, entry) in transcript.entries.iter().enumerate() {
        let entry_chars = entry.content.chars().count();
        if entry_chars > max_entry_chars {
            return Err(ExtractionError::InvalidTranscript(format!(
                "transcript entry {} exceeds maximum content size {}",
                index, max_entry_chars
            )));
        }

        total_chars = total_chars.saturating_add(entry_chars);
        if total_chars > max_total_chars {
            return Err(ExtractionError::InvalidTranscript(format!(
                "transcript total content size exceeds maximum {}",
                max_total_chars
            )));
        }
    }

    Ok(())
}
