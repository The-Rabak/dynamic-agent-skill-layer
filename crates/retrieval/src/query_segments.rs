//! Pure, deterministic prompt segmentation for query-side multi-view priming.
//!
//! Splits a prompt into cheap, deterministic segments without any LLM or network
//! call (T12 hot-path / no-LLM fence). Each segment is a contiguous topic fragment
//! that can be embedded independently; the caller max-fuses scores over all segments
//! so each skill scores by its best-matching segment (curing the topic-density
//! dilution that blurs verbose multi-topic session-start prompts).
//!
//! The segmentation is the query-side analogue of T09's doc-side dense multi-view
//! `fuse_dense_views` (a skill's best-matching view wins). Here the prompt is split
//! and each skill is scored against every segment; the max score wins.
//!
//! Strategy:
//! 1. Split on blank-line (paragraph) boundaries; trim each paragraph; drop empties.
//! 2. Further split any paragraph longer than `max_segment_chars` on sentence
//!    boundaries (`". "` or `'\n'`) so one giant paragraph still yields multiple views.
//! 3. Cap the total at `max_segments` (keep the first N — openings front-load intent).
//! 4. If the result is empty (whitespace-only input), return `[trimmed_prompt]`
//!    (which may be the empty string; length always ≥ 1 so the caller always has
//!    at least one embedding to submit).
//!
//! A prompt with one short paragraph returns exactly `[whole_prompt]`, making the
//! single-segment / Task paths numerically identical.

/// Default maximum number of segments to produce from one prompt.
///
/// Openings front-load intent in the first few paragraphs; beyond 8 segments the
/// marginal topic coverage drops off and the embedding batch cost grows linearly.
pub const DEFAULT_MAX_SEGMENTS: usize = 8;

/// Default per-segment character cap before sentence splitting kicks in.
///
/// 280 chars ≈ two dense Twitter-length sentences — enough for a single topic but
/// short enough that a standard paragraph does not require splitting.
pub const DEFAULT_MAX_SEGMENT_CHARS: usize = 280;

/// Splits a prompt into cheap, deterministic segments for query-side multi-view.
///
/// NO LLM, NO network — pure string work. See the module-level doc for the full
/// strategy. Returns a `Vec<String>` with length ≥ 1 (invariant: never empty).
pub fn segment_prompt(prompt: &str, max_segments: usize, max_segment_chars: usize) -> Vec<String> {
    // Split on blank-line paragraph boundaries; trim; drop empties.
    let paragraphs: Vec<&str> = prompt
        .split("\n\n")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut segments: Vec<String> = Vec::new();

    for paragraph in paragraphs {
        if segments.len() >= max_segments {
            break;
        }
        if paragraph.len() <= max_segment_chars {
            segments.push(paragraph.to_owned());
        } else {
            // Long paragraph: further split on sentence boundaries so one giant
            // paragraph still yields multiple topically-distinct segments.
            let sub = split_on_sentence_boundaries(paragraph, max_segment_chars);
            for s in sub {
                if segments.len() >= max_segments {
                    break;
                }
                segments.push(s);
            }
        }
    }

    if segments.is_empty() {
        // Whitespace-only or fully-empty prompt: always return exactly one segment so
        // the caller always has one text to embed (never an empty batch).
        return vec![prompt.trim().to_owned()];
    }

    segments
}

/// Splits `paragraph` on `". "` and `'\n'` sentence boundaries, accumulating
/// pieces into chunks that stay within `max_chars`. A chunk that exceeds
/// `max_chars` before a boundary is found is kept whole (we never break mid-word
/// because splitting mid-sentence harms embedding quality).
fn split_on_sentence_boundaries(paragraph: &str, max_chars: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut remaining = paragraph;

    while !remaining.is_empty() {
        // Find the next sentence boundary: `. ` (period + space) or `\n`.
        let split_at = remaining
            .find(". ")
            .map(|i| (i + 2, true)) // advance past ". "
            .or_else(|| remaining.find('\n').map(|i| (i + 1, false)));

        match split_at {
            Some((advance, dot_split)) => {
                // Extract the piece that ends at this boundary.
                let piece = if dot_split {
                    // Include the period but not the trailing space in the piece.
                    remaining[..advance - 1].trim()
                } else {
                    remaining[..advance].trim_end_matches('\n').trim()
                };

                let combined_len = if current.is_empty() {
                    piece.len()
                } else {
                    current.len() + 1 + piece.len()
                };

                if combined_len > max_chars && !current.is_empty() {
                    // Flush the accumulated chunk before adding this piece.
                    let flushed = current.trim().to_owned();
                    if !flushed.is_empty() {
                        chunks.push(flushed);
                    }
                    current = piece.to_owned();
                } else if current.is_empty() {
                    current = piece.to_owned();
                } else {
                    current.push(' ');
                    current.push_str(piece);
                }

                remaining = &remaining[advance..];
            }
            None => {
                // No more boundaries — consume the rest.
                let piece = remaining.trim();
                let combined_len = if current.is_empty() {
                    piece.len()
                } else {
                    current.len() + 1 + piece.len()
                };

                if combined_len > max_chars && !current.is_empty() {
                    let flushed = current.trim().to_owned();
                    if !flushed.is_empty() {
                        chunks.push(flushed);
                    }
                    current = piece.to_owned();
                } else if current.is_empty() {
                    current = piece.to_owned();
                } else {
                    current.push(' ');
                    current.push_str(piece);
                }

                remaining = "";
            }
        }
    }

    let last = current.trim().to_owned();
    if !last.is_empty() {
        chunks.push(last);
    }

    // Guard: never return empty (shouldn't happen given a non-empty paragraph).
    if chunks.is_empty() {
        chunks.push(paragraph.trim().to_owned());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_prompt_returns_single_segment_equal_to_whole_prompt() {
        let prompt = "implement auth middleware";
        let segments = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert_eq!(
            segments.len(),
            1,
            "short prompt must yield exactly 1 segment"
        );
        assert_eq!(segments[0], prompt);
    }

    #[test]
    fn multi_paragraph_verbose_prompt_returns_multiple_segments() {
        let prompt = "Starting a new session today.\n\nWe need to implement the authentication system.\n\nAlso need to fix the database migration.";
        let segments = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert!(
            segments.len() > 1,
            "multi-paragraph prompt must yield more than 1 segment; got {}",
            segments.len()
        );
        assert!(
            segments[0].contains("Starting"),
            "first segment must come from the first paragraph"
        );
    }

    #[test]
    fn single_very_long_paragraph_splits_into_multiple_segments() {
        // Three sentences totalling > 280 chars — must split. Each sentence is
        // deliberately padded to keep the total clearly above the 280-char threshold.
        let prompt = "The authentication system needs careful attention to token lifetime management and expiry policy enforcement. We must also consider the session persistence strategy across multiple distributed services and regional data centers. Finally the OAuth flow requires robust error handling with graceful degradation support.";
        assert!(
            prompt.len() > DEFAULT_MAX_SEGMENT_CHARS,
            "test premise: prompt length {} must exceed DEFAULT_MAX_SEGMENT_CHARS {}",
            prompt.len(),
            DEFAULT_MAX_SEGMENT_CHARS
        );
        let segments = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert!(
            segments.len() > 1,
            "long single paragraph must split into multiple segments; got {}",
            segments.len()
        );
    }

    #[test]
    fn max_segments_cap_is_respected() {
        // 20 short paragraphs; cap at 5.
        let prompt = (0..20)
            .map(|i| format!("paragraph {i} with unique content about topic {i}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let cap = 5;
        let segments = segment_prompt(&prompt, cap, DEFAULT_MAX_SEGMENT_CHARS);
        assert!(
            segments.len() <= cap,
            "cap={cap} must be respected; got {} segments",
            segments.len()
        );
    }

    #[test]
    fn whitespace_only_prompt_returns_single_segment() {
        let prompt = "   \n\n   \t  ";
        let segments = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert_eq!(
            segments.len(),
            1,
            "whitespace-only prompt must return exactly 1 segment (len≥1 invariant)"
        );
        // The segment is the trimmed prompt (empty string for all-whitespace input).
        assert_eq!(segments[0], "");
    }

    #[test]
    fn deterministic_same_input_produces_same_output() {
        let prompt = "First paragraph about authentication.\n\nSecond paragraph about database migrations and schema design for the upcoming release.";
        let a = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        let b = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert_eq!(
            a, b,
            "segmentation must be deterministic for identical inputs"
        );
    }

    #[test]
    fn single_short_sentence_is_one_segment() {
        let prompt = "Fix the login bug";
        let segments = segment_prompt(prompt, DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], prompt);
    }

    #[test]
    fn empty_string_returns_single_empty_segment() {
        let segments = segment_prompt("", DEFAULT_MAX_SEGMENTS, DEFAULT_MAX_SEGMENT_CHARS);
        assert_eq!(segments.len(), 1, "empty input must return length-1 result");
        assert_eq!(segments[0], "");
    }
}
