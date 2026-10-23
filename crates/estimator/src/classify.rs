//! Local, deterministic prompt classification.
//!
//! Runs inside the hook process. The prompt is only read in memory; callers
//! keep the returned [`PromptFeatures`] and drop the text. No network, no model.

use runmarq_core::event::{ComplexityBucket, PromptFeatures, TaskCategory};
use sha2::{Digest, Sha256};

struct Rule {
    category: TaskCategory,
    /// (phrase, weight). Single words match on word boundaries; phrases match as substrings.
    terms: &'static [(&'static str, f32)],
}

const RULES: &[Rule] = &[
    Rule {
        category: TaskCategory::SmallEdit,
        terms: &[
            ("typo", 3.0),
            ("rename", 1.5),
            ("tweak", 2.0),
            ("one-line", 3.0),
            ("one line", 2.5),
            ("quick", 1.0),
            ("small change", 2.5),
            ("change the text", 2.5),
            ("update the copy", 2.5),
            ("wording", 2.0),
            ("comment", 1.0),
            ("bump the version", 2.5),
            ("fix the import", 2.5),
        ],
    },
    Rule {
        category: TaskCategory::BugFix,
        terms: &[
            ("bug", 2.5),
            ("fix", 1.5),
            ("error", 1.5),
            ("crash", 2.5),
            ("crashes", 2.5),
            ("broken", 2.0),
            ("failing", 1.5),
            ("exception", 2.0),
            ("doesn't work", 2.5),
            ("does not work", 2.5),
            ("not working", 2.5),
            ("regression", 2.5),
            ("traceback", 2.5),
            ("stack trace", 2.5),
            ("panic", 2.0),
            ("wrong", 1.0),
            ("issue", 1.0),
        ],
    },
    Rule {
        category: TaskCategory::FeatureImplementation,
        terms: &[
            ("implement", 2.5),
            ("add", 1.0),
            ("feature", 2.5),
            ("build", 1.0),
            ("create", 1.0),
            ("support for", 2.0),
            ("endpoint", 2.0),
            ("integrate", 2.0),
            ("integration", 1.5),
            ("new", 0.5),
            ("authentication", 2.0),
            ("api", 1.0),
        ],
    },
    Rule {
        category: TaskCategory::UiDevelopment,
        terms: &[
            ("ui", 2.0),
            ("css", 2.5),
            ("layout", 2.0),
            ("component", 1.5),
            ("page", 1.0),
            ("button", 2.0),
            ("style", 1.5),
            ("styling", 2.0),
            ("tailwind", 2.5),
            ("responsive", 2.5),
            ("frontend", 2.0),
            ("modal", 2.0),
            ("animation", 2.0),
            ("dark mode", 2.5),
            ("design", 1.0),
        ],
    },
    Rule {
        category: TaskCategory::Refactoring,
        terms: &[
            ("refactor", 3.0),
            ("clean up", 2.5),
            ("cleanup", 2.5),
            ("restructure", 3.0),
            ("simplify", 2.0),
            ("extract", 2.0),
            ("reorganize", 2.5),
            ("deduplicate", 2.5),
            ("split", 1.0),
            ("decouple", 2.5),
        ],
    },
    Rule {
        category: TaskCategory::Testing,
        terms: &[
            ("test", 1.5),
            ("tests", 2.0),
            ("unit test", 3.0),
            ("unit tests", 3.0),
            ("coverage", 2.5),
            ("e2e", 2.5),
            ("integration test", 3.0),
            ("pytest", 2.5),
            ("jest", 2.5),
            ("vitest", 2.5),
            ("playwright", 2.0),
            ("snapshot", 1.0),
        ],
    },
    Rule {
        category: TaskCategory::DependencyWork,
        terms: &[
            ("upgrade", 2.0),
            ("dependency", 2.5),
            ("dependencies", 2.5),
            ("package", 1.0),
            ("bump", 2.0),
            ("lockfile", 2.5),
            ("npm install", 2.5),
            ("pnpm", 1.5),
            ("cargo update", 3.0),
            ("pip install", 2.5),
            ("vulnerab", 2.0),
        ],
    },
    Rule {
        category: TaskCategory::RepositoryAnalysis,
        terms: &[
            ("explain", 2.5),
            ("how does", 2.5),
            ("where is", 2.5),
            ("where are", 2.5),
            ("what does", 2.5),
            ("understand", 2.0),
            ("analyze", 2.0),
            ("analyse", 2.0),
            ("review", 2.0),
            ("summarize", 2.5),
            ("overview", 2.0),
            ("walk me through", 3.0),
            ("find", 1.0),
            ("audit", 2.0),
        ],
    },
    Rule {
        category: TaskCategory::Research,
        terms: &[
            ("research", 3.0),
            ("compare", 2.0),
            ("investigate", 2.0),
            ("look up", 2.0),
            ("documentation", 1.5),
            ("best way", 2.0),
            ("options for", 2.0),
            ("alternatives", 2.0),
            ("pros and cons", 3.0),
        ],
    },
    Rule {
        category: TaskCategory::Migration,
        terms: &[
            ("migrate", 3.0),
            ("migration", 2.5),
            ("port to", 3.0),
            ("convert", 2.0),
            ("move from", 2.5),
            ("rewrite in", 3.0),
            ("upgrade from", 2.5),
            ("switch from", 2.5),
        ],
    },
];

const SCOPE_WIDENERS: &[&str] = &[
    "all",
    "entire",
    "every",
    "whole",
    "across",
    "codebase",
    "end-to-end",
    "complete",
    "full",
    "everything",
    "throughout",
];
const SCOPE_NARROWERS: &[&str] = &[
    "just", "only", "quick", "small", "simple", "tiny", "single", "minor", "slight",
];
const FILE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "swift", "rb", "php",
    "c", "h", "cpp", "hpp", "cs", "css", "scss", "html", "vue", "svelte", "json", "toml", "yaml",
    "yml", "md", "sql", "sh", "ps1", "lock",
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '\''
}

/// Counts occurrences of `term` in `text`. Single words must sit on word boundaries.
fn count_term(text: &str, term: &str) -> usize {
    if term.contains(' ') || term.ends_with("ab") {
        return text.matches(term).count();
    }
    let mut n = 0;
    let mut start = 0;
    while let Some(pos) = text[start..].find(term) {
        let abs = start + pos;
        let before = text[..abs].chars().next_back();
        let after = text[abs + term.len()..].chars().next();
        let boundary_before = before.is_none_or(|c| !is_word_char(c));
        // Allow plural/verb suffixes like "fixes", "tests", "crashed".
        let rest = &text[abs + term.len()..];
        let suffix_len = ["es", "s", "ed", "ing"]
            .iter()
            .find(|s| rest.starts_with(**s))
            .map(|s| s.len())
            .unwrap_or(0);
        let after_suffix = text[abs + term.len() + suffix_len..].chars().next();
        let boundary_after =
            after.is_none_or(|c| !is_word_char(c)) || after_suffix.is_none_or(|c| !is_word_char(c));
        if boundary_before && boundary_after {
            n += 1;
        }
        start = abs + term.len();
    }
    n
}

fn strip_code_blocks(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut in_block = false;
    let mut had_block = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            had_block = true;
            continue;
        }
        if !in_block {
            out.push_str(line);
            out.push('\n');
        }
    }
    (out, had_block)
}

fn looks_like_file(token: &str) -> bool {
    let t = token.trim_matches(|c: char| "`'\"()[]{},;:<>".contains(c));
    if t.len() < 3 || t.len() > 260 || t.starts_with("http") {
        return false;
    }
    if let Some((stem, ext)) = t.rsplit_once('.') {
        if !stem.is_empty() && FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    (t.contains('/') || t.contains('\\'))
        && t.chars().filter(|c| *c == '/' || *c == '\\').count() >= 1
        && !t.contains(' ')
        && t.chars().any(|c| c.is_alphabetic())
}

fn count_list_items(text: &str) -> u32 {
    text.lines()
        .filter(|l| {
            let l = l.trim_start();
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.starts_with("• ")
                || l.split_once(['.', ')']).is_some_and(|(n, rest)| {
                    !n.is_empty()
                        && n.len() <= 3
                        && n.chars().all(|c| c.is_ascii_digit())
                        && rest.starts_with(' ')
                })
        })
        .count() as u32
}

pub fn complexity_bucket(score: f32) -> ComplexityBucket {
    match score {
        s if s < 0.2 => ComplexityBucket::Trivial,
        s if s < 0.4 => ComplexityBucket::Small,
        s if s < 0.6 => ComplexityBucket::Medium,
        s if s < 0.8 => ComplexityBucket::Large,
        _ => ComplexityBucket::VeryLarge,
    }
}

/// Classifies a prompt. `salt` enables a fingerprint for recognizing repeated
/// prompts; pass `None` to skip it.
pub fn classify(prompt: &str, salt: Option<&[u8]>) -> PromptFeatures {
    // Bound the work regardless of input size.
    let bounded: String = prompt.chars().take(20_000).collect();
    let lower = bounded.to_lowercase();
    let (prose, _had_code) = strip_code_blocks(&lower);

    let word_count = prose.split_whitespace().count() as u32;
    let list_items = count_list_items(&prose);
    let file_mentions = bounded
        .split_whitespace()
        .filter(|t| looks_like_file(t))
        .count()
        .min(200) as u32;
    let mentions_tests = ["test", "tests", "spec", "coverage"]
        .iter()
        .any(|t| count_term(&prose, t) > 0);

    let mut scores: Vec<(TaskCategory, f32)> = RULES
        .iter()
        .map(|r| {
            let s: f32 = r
                .terms
                .iter()
                .map(|(term, w)| count_term(&prose, term).min(3) as f32 * w)
                .sum();
            (r.category, s)
        })
        .collect();

    // Short prompts with edit verbs are usually small edits.
    if word_count <= 12 {
        if let Some(e) = scores
            .iter_mut()
            .find(|(c, _)| *c == TaskCategory::SmallEdit)
        {
            e.1 += 1.0;
        }
    }
    scores.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let (top_cat, top_score) = scores[0];
    let strong = scores.iter().filter(|(_, s)| *s >= 3.0).count();

    let then_count = count_term(&prose, "then")
        + prose.matches("and also").count()
        + prose.matches("after that").count();
    let category = if list_items >= 3 || (strong >= 3 && word_count > 60) || then_count >= 3 {
        TaskCategory::MultiStep
    } else if top_score < 1.5 {
        TaskCategory::Unknown
    } else {
        top_cat
    };

    let widen = SCOPE_WIDENERS
        .iter()
        .map(|w| count_term(&prose, w).min(2))
        .sum::<usize>() as f32;
    let narrow = SCOPE_NARROWERS
        .iter()
        .map(|w| count_term(&prose, w).min(2))
        .sum::<usize>() as f32;
    let raw = 0.12 * ((word_count as f32) + 1.0).ln()
        + 0.06 * (list_items as f32).min(10.0)
        + 0.04 * (file_mentions as f32).min(10.0)
        + 0.06 * widen.min(5.0)
        - 0.07 * narrow.min(4.0)
        + if category == TaskCategory::MultiStep {
            0.1
        } else {
            0.0
        };
    let complexity_score = raw.clamp(0.0, 1.0);

    let fingerprint = salt.map(|salt| {
        let normalized = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut h = Sha256::new();
        h.update(salt);
        h.update(normalized.as_bytes());
        hex::encode(&h.finalize()[..16])
    });

    PromptFeatures {
        category,
        complexity: complexity_bucket(complexity_score),
        complexity_score,
        word_count,
        list_items,
        file_mentions,
        mentions_tests,
        fingerprint,
        sketch: None,
    }
}

/// Builds a short, opt-in task title: the first meaningful line, stripped of
/// markdown, with secret-looking tokens redacted, truncated to 80 characters.
pub fn derive_title(prompt: &str) -> Option<String> {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))?;
    let cleaned: String = line
        .trim_start_matches(['#', '>', '-', '*', ' '])
        .split_whitespace()
        .map(|tok| if looks_secret(tok) { "•••" } else { tok })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let mut out: String = cleaned.chars().take(80).collect();
    if cleaned.chars().count() > 80 {
        out = out.trim_end().to_string();
        out.push('…');
    }
    Some(out)
}

pub const SKETCH_DIMS: usize = 256;

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do", "does", "for", "from",
    "have", "how", "i", "if", "in", "into", "is", "it", "its", "me", "my", "of", "on", "or",
    "please", "so", "that", "the", "then", "there", "this", "to", "up", "we", "what", "when",
    "where", "which", "with", "you", "your", "should", "would", "could", "make", "sure", "also",
    "just", "now", "all", "any", "some", "not", "no", "yes", "will", "was", "were",
];

fn seeded_hash(seed: u64, a: &str, b: Option<&str>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    let mut feed = |bytes: &[u8]| {
        for &x in bytes {
            h ^= x as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(a.as_bytes());
    if let Some(b) = b {
        feed(&[0x1f]);
        feed(b.as_bytes());
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

/// Salted, feature-hashed term vector of a prompt (unigrams and bigrams,
/// stopwords removed), L2-normalized and quantized to `i8`. The text cannot
/// be read back from it; it only tells which prompts use similar words.
pub fn sketch(prompt: &str, salt: &[u8]) -> Vec<i8> {
    let seed = u64::from_le_bytes(Sha256::digest(salt)[..8].try_into().expect("8 bytes"));
    let lower: String = prompt
        .chars()
        .take(20_000)
        .collect::<String>()
        .to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.len() >= 2 && w.len() <= 30 && !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| !STOPWORDS.contains(w))
        .collect();
    let mut v = [0f32; SKETCH_DIMS];
    let mut add = |h: u64, w: f32| {
        let i = (h % SKETCH_DIMS as u64) as usize;
        v[i] += if h >> 63 == 0 { w } else { -w };
    };
    for w in &words {
        add(seeded_hash(seed, w, None), 1.0);
    }
    for pair in words.windows(2) {
        add(seeded_hash(seed, pair[0], Some(pair[1])), 0.6);
    }
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    if max == 0.0 {
        return vec![0; SKETCH_DIMS];
    }
    v.iter().map(|x| (x / max * 127.0).round() as i8).collect()
}

/// Cosine similarity of two sketches, in [-1, 1]. Zero vectors score 0.
pub fn sketch_similarity(a: &[i8], b: &[i8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0i64, 0i64, 0i64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (*x as i64, *y as i64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0 || nb == 0 {
        return 0.0;
    }
    (dot as f64 / ((na as f64).sqrt() * (nb as f64).sqrt())) as f32
}

fn looks_secret(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
    let prefixes = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "github_pat_",
        "xox",
        "AKIA",
        "AIza",
        "eyJ",
        "glpat-",
        "npm_",
    ];
    if prefixes.iter().any(|p| t.starts_with(p)) && t.len() >= 12 {
        return true;
    }
    if tok.contains('=') && tok.len() > 16 {
        let (k, v) = tok.split_once('=').unwrap_or(("", ""));
        let k = k.to_ascii_lowercase();
        if ["key", "token", "secret", "password", "passwd", "pwd"]
            .iter()
            .any(|s| k.contains(s))
            && !v.is_empty()
        {
            return true;
        }
    }
    let alnum = t.chars().filter(|c| c.is_ascii_alphanumeric()).count();
    t.len() >= 24
        && alnum as f32 / t.len() as f32 > 0.9
        && t.chars().any(|c| c.is_ascii_digit())
        && t.chars().any(|c| c.is_ascii_uppercase())
        && t.chars().any(|c| c.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(p: &str) -> TaskCategory {
        classify(p, None).category
    }

    #[test]
    fn categories() {
        assert_eq!(cat("fix the typo in the README"), TaskCategory::SmallEdit);
        assert_eq!(
            cat("The login page crashes with a null pointer exception when the session expires"),
            TaskCategory::BugFix
        );
        assert_eq!(
            cat("Implement OAuth authentication with a new /auth endpoint"),
            TaskCategory::FeatureImplementation
        );
        assert_eq!(
            cat("Make the sidebar layout responsive and fix the button styling in dark mode"),
            TaskCategory::UiDevelopment
        );
        assert_eq!(
            cat("Refactor the storage module and extract the query builder"),
            TaskCategory::Refactoring
        );
        assert_eq!(
            cat("Write unit tests for the parser and increase coverage"),
            TaskCategory::Testing
        );
        assert_eq!(
            cat("Upgrade all dependencies and regenerate the lockfile"),
            TaskCategory::DependencyWork
        );
        assert_eq!(
            cat("Explain how does the auth middleware work?"),
            TaskCategory::RepositoryAnalysis
        );
        assert_eq!(
            cat("Migrate the project from webpack, port to vite"),
            TaskCategory::Migration
        );
        assert_eq!(cat("hello"), TaskCategory::Unknown);
        assert_eq!(
            cat("Please do the following:\n1. add a table\n2. write the API\n3. add tests\n4. update docs"),
            TaskCategory::MultiStep
        );
    }

    #[test]
    fn word_boundaries() {
        assert_eq!(count_term("prefix suffix", "fix"), 0);
        assert_eq!(count_term("fix this, then fixes that", "fix"), 2);
        assert_eq!(count_term("the ui is off", "ui"), 1);
        assert_eq!(count_term("build guide", "ui"), 0);
    }

    #[test]
    fn complexity_orders_sensibly() {
        let small = classify("just fix the typo", None);
        let big = classify(
            "Refactor the entire codebase to use the new storage layer across all services, update every test, \
             and make sure src/storage/lib.rs, src/api/routes.ts and src/app/main.py are consistent. Then update \
             the docs and the CI pipeline. Everything should be complete end-to-end.",
            None,
        );
        assert!(small.complexity_score < big.complexity_score);
        assert!(big.file_mentions >= 3);
        assert!(big.complexity >= ComplexityBucket::Medium);
        assert!(small.complexity <= ComplexityBucket::Small);
    }

    #[test]
    fn fingerprint_is_salted_and_normalized() {
        let a = classify("Fix  the bug", Some(b"salt1"))
            .fingerprint
            .unwrap();
        let b = classify("fix the   bug", Some(b"salt1"))
            .fingerprint
            .unwrap();
        let c = classify("fix the bug", Some(b"salt2")).fingerprint.unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn code_blocks_do_not_drive_category() {
        let p = "Why is this slow?\n```\nfn test_migrate_refactor() {}\n```";
        assert_ne!(cat(p), TaskCategory::Migration);
    }

    #[test]
    fn titles_redact_secrets() {
        let t = derive_title("## Use key sk-abcdefghijklmnop1234 to call the API\nmore").unwrap();
        assert!(!t.contains("sk-abc"), "{t}");
        assert!(t.starts_with("Use key"));
        let t = derive_title("set API_TOKEN=supersecretvalue123 in env").unwrap();
        assert!(!t.contains("supersecret"));
        let long = "a ".repeat(100);
        assert!(derive_title(&long).unwrap().chars().count() <= 81);
        assert_eq!(derive_title("   \n  "), None);
    }

    #[test]
    fn sketches_find_similar_prompts() {
        let salt = b"salt";
        let a = sketch("Fix the failing login test in the auth module", salt);
        let b = sketch("The login test in auth is failing, please fix it", salt);
        let c = sketch("Write a changelog entry for the new pricing page", salt);
        assert_eq!(a.len(), SKETCH_DIMS);
        assert!(
            sketch_similarity(&a, &b) > 0.5,
            "{}",
            sketch_similarity(&a, &b)
        );
        assert!(
            sketch_similarity(&a, &c) < 0.3,
            "{}",
            sketch_similarity(&a, &c)
        );
        assert!((sketch_similarity(&a, &a) - 1.0).abs() < 1e-6);
        let other_salt = sketch("Fix the failing login test in the auth module", b"other");
        assert!(
            sketch_similarity(&a, &other_salt) < 0.5,
            "different salts give unrelated vectors"
        );
        assert_eq!(sketch_similarity(&sketch("the and of", salt), &a), 0.0);
    }

    #[test]
    fn huge_input_is_bounded() {
        let p = "fix bug ".repeat(200_000);
        let f = classify(&p, None);
        assert!(f.word_count <= 20_000);
    }
}
