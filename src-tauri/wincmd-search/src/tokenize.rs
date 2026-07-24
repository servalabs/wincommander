// SPDX-License-Identifier: AGPL-3.0-or-later
//! Code-aware tokenizer for the keyword index.
//!
//! tantivy's default tokenizer only splits on non-alphanumeric boundaries, so
//! an ID like `XXXXXXXX2982` indexes as ONE atomic token and a bare `2982`
//! query can never match it. `CodeTokenizer` emits alphanumeric runs that are
//! additionally split at alpha↔digit transitions (`XXXXXXXX2982` →
//! [`XXXXXXXX`, `2982`]). The query side runs through the same analyzer, so a
//! full `XXXXXXXX2982` query becomes the equivalent phrase and still matches.
//!
//! `wc_code` also folds diacritics/ligatures to ASCII: PDF extraction commonly
//! yields ligature codepoints (`office` -> `oﬃce`) and some sources emit
//! NFD-normalized text (combining accents), and a plain-ASCII query must
//! still find both.

use tantivy::tokenizer::{
    AsciiFoldingFilter, LowerCaser, RawTokenizer, RemoveLongFilter, TextAnalyzer, Token,
    TokenFilter, TokenStream, Tokenizer,
};

/// Analyzer name for `name`/`title`/`body` (code-aware split + lowercase).
pub const CODE_TOKENIZER: &str = "wc_code";
/// Analyzer name for `name_lc` (whole value as one lowercased term — the
/// substring-match rung regex-scans this field's term dictionary).
pub const RAW_LC_TOKENIZER: &str = "wc_raw_lc";

/// Register both custom analyzers on `index`. Must run after every
/// `Index::open`/`create` and before any indexing or query parsing.
pub fn register_tokenizers(index: &tantivy::Index) {
    index.tokenizers().register(
        CODE_TOKENIZER,
        TextAnalyzer::builder(CodeTokenizer::default())
            // A pure-digit run (an ID with no alpha↔digit boundary to split
            // at, e.g. an enrollment/invoice/phone number) is otherwise one
            // atomic token — a query for its first N digits can never match
            // it. Expand it into left-anchored prefixes before the length
            // cap below (see `DigitPrefixFilter`).
            .filter(DigitPrefixFilter)
            // Parity with tantivy's default analyzer intent (it uses 40); 64
            // keeps long serials searchable while dropping minified-blob runs.
            .filter(RemoveLongFilter::limit(64))
            .filter(LowerCaser)
            // KT: PDF extractors emit ligatures ("office" -> "oﬃce") and text
            // can arrive NFD-normalized (combining accents); without folding,
            // a plain-ASCII query silently misses those docs. Must run after
            // LowerCaser so its fold table (which is lowercase-keyed) matches.
            .filter(AsciiFoldingFilter)
            .build(),
    );
    index.tokenizers().register(
        RAW_LC_TOKENIZER,
        TextAnalyzer::builder(RawTokenizer::default())
            .filter(LowerCaser)
            // Fold accents/ligatures too, same as `wc_code` — otherwise the
            // filename SUBSTRING rung (which regex-scans this field) can't
            // match "cafe" against a stored "café.txt" even though the
            // tokenized name/body fields already fold both sides. The needle
            // must be folded through this SAME analyzer before building the
            // regex (see `append_name_substring_hits`) or the two sides drift.
            .filter(AsciiFoldingFilter)
            .build(),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Alpha,
    Digit,
}

fn classify(c: char) -> Option<CharClass> {
    if c.is_alphabetic() {
        Some(CharClass::Alpha)
    } else if c.is_numeric() {
        Some(CharClass::Digit)
    } else {
        None
    }
}

/// Emits maximal same-class (alphabetic | numeric) character runs as tokens.
#[derive(Clone, Default)]
pub struct CodeTokenizer {
    token: Token,
}

pub struct CodeTokenStream<'a> {
    text: &'a str,
    chars: std::str::CharIndices<'a>,
    token: &'a mut Token,
}

impl Tokenizer for CodeTokenizer {
    type TokenStream<'a> = CodeTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> CodeTokenStream<'a> {
        self.token = Token::default();
        // wrapping_add(1) in the first advance() yields position 0.
        self.token.position = usize::MAX;
        CodeTokenStream {
            text,
            chars: text.char_indices(),
            token: &mut self.token,
        }
    }
}

impl TokenStream for CodeTokenStream<'_> {
    fn advance(&mut self) -> bool {
        self.token.text.clear();
        self.token.position = self.token.position.wrapping_add(1);
        for (start, c) in self.chars.by_ref() {
            let Some(class) = classify(c) else { continue };
            let mut end = start + c.len_utf8();
            loop {
                // Peek without consuming a char of a different class.
                let mut peek = self.chars.clone();
                match peek.next() {
                    Some((idx, nc)) if classify(nc) == Some(class) => {
                        self.chars = peek;
                        end = idx + nc.len_utf8();
                    }
                    _ => break,
                }
            }
            self.token.offset_from = start;
            self.token.offset_to = end;
            self.token.text.push_str(&self.text[start..end]);
            return true;
        }
        false
    }

    fn token(&self) -> &Token {
        self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        self.token
    }
}

/// Digit runs shorter than this are left as a single token — too short to
/// bother expanding (e.g. a bare "2024" year), and expanding down to 1-3
/// digit prefixes would make nearly every numeric token in a corpus match,
/// which is noise rather than a useful partial-ID search.
const MIN_EXPAND_LEN: usize = 5;
/// Shortest prefix ever emitted as its own token — mirrors `MIN_EXPAND_LEN`'s
/// reasoning: a 1-3 digit query is too unselective to be a meaningful ID
/// lookup.
const MIN_PREFIX_LEN: usize = 4;
/// Digit runs longer than this are left unexpanded too — guards against a
/// huge incidental numeric blob (not a real-world ID) generating an
/// unbounded number of prefix postings for one token.
const MAX_EXPAND_LEN: usize = 32;

/// Expands a long pure-ASCII-digit token into its left-anchored (prefix)
/// substrings, in addition to the full token, so a query for the first N
/// digits of a long numeric ID — an enrollment number, invoice number,
/// account or phone number — embedded in a file's name or content is still
/// findable even though it shares no alpha↔digit boundary with `CodeTokenizer`
/// to split at. All emitted prefixes carry the ORIGINAL token's offsets and
/// position (mirrors tantivy's own `SplitCompoundWords`: alternate forms of
/// one input token co-locate at that token's position, not sequential
/// positions), so a query hit still highlights the whole number, and phrase
/// queries spanning this position are unaffected.
///
/// A token that isn't all ASCII digits, or whose length falls outside
/// `MIN_EXPAND_LEN..=MAX_EXPAND_LEN`, passes through unchanged (byte-slicing
/// prefixes is only safe because ASCII digits are always 1 byte each).
#[derive(Clone, Default)]
pub struct DigitPrefixFilter;

impl TokenFilter for DigitPrefixFilter {
    type Tokenizer<T: Tokenizer> = DigitPrefixFilterWrapper<T>;

    fn transform<T: Tokenizer>(self, tokenizer: T) -> DigitPrefixFilterWrapper<T> {
        DigitPrefixFilterWrapper { inner: tokenizer }
    }
}

#[derive(Clone)]
pub struct DigitPrefixFilterWrapper<T> {
    inner: T,
}

impl<T: Tokenizer> Tokenizer for DigitPrefixFilterWrapper<T> {
    type TokenStream<'a> = DigitPrefixTokenStream<T::TokenStream<'a>>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        DigitPrefixTokenStream {
            tail: self.inner.token_stream(text),
            queue: Vec::new(),
            current: Token::default(),
        }
    }
}

pub struct DigitPrefixTokenStream<T> {
    tail: T,
    // Queued shorter-prefix tokens still owed for the CURRENT source token,
    // shortest pushed last so `pop()` (LIFO) yields longest-prefix-first —
    // iteration order only, doesn't affect search correctness.
    queue: Vec<Token>,
    current: Token,
}

impl<T: TokenStream> TokenStream for DigitPrefixTokenStream<T> {
    fn advance(&mut self) -> bool {
        if let Some(t) = self.queue.pop() {
            self.current = t;
            return true;
        }
        if !self.tail.advance() {
            return false;
        }
        let base = self.tail.token();
        let len = base.text.len();
        if base.text.bytes().all(|b| b.is_ascii_digit())
            && (MIN_EXPAND_LEN..=MAX_EXPAND_LEN).contains(&len)
        {
            for plen in (MIN_PREFIX_LEN..len).rev() {
                self.queue.push(Token {
                    text: base.text[..plen].to_owned(),
                    ..base.clone()
                });
            }
        }
        self.current = base.clone();
        true
    }

    fn token(&self) -> &Token {
        &self.current
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<String> {
        let mut tok = CodeTokenizer::default();
        let mut stream = tok.token_stream(text);
        let mut out = Vec::new();
        while stream.advance() {
            out.push(stream.token().text.clone());
        }
        out
    }

    #[test]
    fn splits_alpha_digit_boundaries() {
        assert_eq!(tokens("XXXXXXXX2982"), vec!["XXXXXXXX", "2982"]);
        assert_eq!(
            tokens("INV2982-final.pdf"),
            vec!["INV", "2982", "final", "pdf"]
        );
        assert_eq!(tokens("mp3 file"), vec!["mp", "3", "file"]);
    }

    #[test]
    fn plain_words_unchanged() {
        assert_eq!(
            tokens("the quarterly budget"),
            vec!["the", "quarterly", "budget"]
        );
    }

    #[test]
    fn separators_and_empty() {
        assert_eq!(tokens("a_b-c.d"), vec!["a", "b", "c", "d"]);
        assert!(tokens("").is_empty());
        assert!(tokens("--- ___").is_empty());
    }

    #[test]
    fn positions_are_sequential_and_offsets_slice_source() {
        let text = "report2982.txt";
        let mut tok = CodeTokenizer::default();
        let mut stream = tok.token_stream(text);
        let mut expected_pos = 0;
        while stream.advance() {
            let t = stream.token();
            assert_eq!(t.position, expected_pos);
            assert_eq!(&text[t.offset_from..t.offset_to], t.text);
            expected_pos += 1;
        }
        assert_eq!(expected_pos, 3, "report / 2982 / txt");
    }

    #[test]
    fn unicode_runs_stay_whole() {
        // Non-ASCII letters are Alpha-class; digits split off as usual.
        assert_eq!(tokens("наряд2982"), vec!["наряд", "2982"]);
    }

    /// Runs text through the exact `wc_code` filter chain registered in
    /// `register_tokenizers` (CodeTokenizer → DigitPrefixFilter →
    /// RemoveLongFilter → LowerCaser → AsciiFoldingFilter), so these tests pin
    /// the analyzer as queries/indexing actually see it, not just the bare
    /// `CodeTokenizer`.
    fn code_analyzer_tokens(text: &str) -> Vec<String> {
        let mut analyzer = TextAnalyzer::builder(CodeTokenizer::default())
            .filter(DigitPrefixFilter)
            .filter(RemoveLongFilter::limit(64))
            .filter(LowerCaser)
            .filter(AsciiFoldingFilter)
            .build();
        let mut stream = analyzer.token_stream(text);
        let mut out = Vec::new();
        while stream.advance() {
            out.push(stream.token().text.clone());
        }
        out
    }

    #[test]
    fn ligature_folds_to_ascii_expansion() {
        // PDF extractors emit ligature codepoints (e.g. "office" -> "oﬃce").
        // U+FB03 is alphabetic, so CodeTokenizer keeps it inside the token;
        // AsciiFoldingFilter then expands it to "ffi".
        assert_eq!(code_analyzer_tokens("o\u{FB03}ce"), vec!["office"]);
    }

    #[test]
    fn nfd_combining_accent_splits_but_base_letters_survive() {
        // KT: a combining acute (U+0301) is NOT alphabetic, so CodeTokenizer
        // splits the token there instead of folding through it — NFD text
        // still needs the base-letter token to remain findable.
        assert_eq!(code_analyzer_tokens("cafe\u{0301}"), vec!["cafe"]);
    }

    #[test]
    fn nfc_accented_char_folds_to_ascii() {
        // NFC "café" (U+00E9 = precomposed é) is one alphabetic char that
        // AsciiFoldingFilter maps straight to "e".
        assert_eq!(code_analyzer_tokens("caf\u{E9}"), vec!["cafe"]);
    }

    // ── DigitPrefixFilter (the "23002170" bug: a long pure-digit ID has no
    // alpha↔digit boundary for CodeTokenizer to split at, so only the whole
    // run was ever a searchable token) ─────────────────────────────────────

    #[test]
    fn long_digit_run_expands_into_prefixes_and_keeps_full_token() {
        // "23002170110112" (14 digits, an enrollment number) must remain
        // findable in full AND by any prefix down to 4 digits.
        let toks = code_analyzer_tokens("23002170110112");
        assert!(toks.contains(&"23002170110112".to_string()), "{toks:?}");
        assert!(toks.contains(&"23002170".to_string()), "{toks:?}");
        assert!(toks.contains(&"2300".to_string()), "{toks:?}");
        // Every emitted token must itself be a genuine prefix of the run.
        for t in &toks {
            assert!("23002170110112".starts_with(t.as_str()), "{toks:?}");
        }
    }

    #[test]
    fn prefix_shorter_than_min_prefix_len_is_not_emitted() {
        // MIN_PREFIX_LEN is 4 — a 3-digit or shorter prefix is too
        // unselective to index as its own token.
        let toks = code_analyzer_tokens("23002170110112");
        assert!(!toks.contains(&"230".to_string()), "{toks:?}");
        assert!(!toks.contains(&"2".to_string()), "{toks:?}");
    }

    #[test]
    fn short_digit_run_is_not_expanded() {
        // Below MIN_EXPAND_LEN (5) — a bare year like "2024" stays one token,
        // not four (2, 20, 202, 2024), which would be pure noise.
        assert_eq!(code_analyzer_tokens("2024"), vec!["2024"]);
    }

    #[test]
    fn run_at_min_expand_len_gets_one_extra_prefix() {
        // Exactly 5 digits: only the 4-digit prefix is added alongside the
        // full token — MIN_PREFIX_LEN..len is 4..5, i.e. just "1234".
        let mut toks = code_analyzer_tokens("12345");
        toks.sort();
        assert_eq!(toks, vec!["1234".to_string(), "12345".to_string()]);
    }

    #[test]
    fn digit_run_past_max_expand_len_is_left_whole() {
        // 40 digits exceeds MAX_EXPAND_LEN (32) — must pass through as ONE
        // token, not explode into dozens of prefix postings.
        let long_run = "1".repeat(40);
        let toks = code_analyzer_tokens(&long_run);
        assert_eq!(toks, vec![long_run]);
    }

    #[test]
    fn alpha_digit_boundary_split_runs_still_expand_independently() {
        // "INV23002170110112" splits at the alpha/digit boundary first (INV /
        // 23002170110112); the digit half then expands like any other run.
        let toks = code_analyzer_tokens("INV23002170110112");
        assert!(toks.contains(&"inv".to_string()), "{toks:?}");
        assert!(toks.contains(&"23002170".to_string()), "{toks:?}");
    }

    #[test]
    fn alphabetic_tokens_are_never_expanded() {
        let toks = code_analyzer_tokens("quarterly");
        assert_eq!(toks, vec!["quarterly"]);
    }
}
