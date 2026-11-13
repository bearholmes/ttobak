use std::path::Path;
use tokenizers::Tokenizer;
use ttobak_core::{parse_keyword_list, KeywordUsage, KEYWORD_TOKEN_LIMIT};

/// 한국어 문장부호·띄어쓰기 유도를 위한 기본 컨디셔닝 프롬프트
pub const BASE_KO_PROMPT: &str =
    "안녕하세요. 오늘 회의는 몇 시에 시작하나요? 네, 3시에 시작합니다.";

/// 무음·초단문 구간에서 초기 프롬프트가 에코(환각)될 때 제거할 조각 목록
pub const HINT_FRAGMENTS: [&str; 3] = [
    "안녕하세요.",
    "오늘 회의는 몇 시에 시작하나요?",
    "네, 3시에 시작합니다.",
];

#[derive(Debug, Clone)]
pub struct ConditionedPrompts {
    /// Tier 1: 자주 쓰는 말 포함 프롬프트 (키워드가 없으면 `None`)
    pub tier1_with_keywords: Option<String>,
    /// Tier 2: 순수 한국어 문장부호 유도 기본 프롬프트
    pub tier2_base: String,
    pub usage: KeywordUsage,
}

/// Whisper BPE 토크나이저 래퍼 (`tokenizer.json` 미설치 초기 상태에서는 바이트 기반 추정기로 즉시 동작)
pub struct PromptTokenizer {
    inner: Option<Tokenizer>,
}

impl Default for PromptTokenizer {
    fn default() -> Self {
        Self { inner: None }
    }
}

impl PromptTokenizer {
    pub fn from_file_optional(path: &Path) -> Self {
        let inner = if path.exists() {
            Tokenizer::from_file(path).ok()
        } else {
            None
        };
        Self { inner }
    }

    pub fn is_exact(&self) -> bool {
        self.inner.is_some()
    }

    /// 주어진 문자열의 Whisper BPE 토큰 개수를 계산합니다.
    pub fn count_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        if let Some(tok) = &self.inner {
            if let Ok(enc) = tok.encode(text, false) {
                return enc.len();
            }
        }
        estimate_whisper_bpe_tokens(text)
    }

    /// 용어 사전 원문을 파싱해 `KEYWORD_TOKEN_LIMIT`(80 토큰) 한도 내에서 사용 가능한 키워드와 잔여 토큰을 계산합니다.
    pub fn analyze_keywords(&self, raw_keywords: &str) -> KeywordUsage {
        let all_keywords = parse_keyword_list(raw_keywords);
        if all_keywords.is_empty() {
            return KeywordUsage::default();
        }

        let mut usable = Vec::new();
        let mut usable_tokens = 0usize;

        for kw in &all_keywords {
            let mut candidate = usable.clone();
            candidate.push(kw.clone());
            let joined = format!(" {}.", candidate.join(", "));
            let tok_len = self.count_tokens(&joined);
            if tok_len <= KEYWORD_TOKEN_LIMIT {
                usable = candidate;
                usable_tokens = tok_len;
            } else {
                // 한도를 초과하는 첫 지점 이후는 프롬프트에 넣지 않음 (총 입력 토큰 수는 전체 기준으로 보고)
                let full_joined = format!(" {}.", all_keywords.join(", "));
                let total_tokens = self.count_tokens(&full_joined).max(KEYWORD_TOKEN_LIMIT + 1);
                return KeywordUsage {
                    tokens: total_tokens,
                    limit: KEYWORD_TOKEN_LIMIT,
                    remaining_tokens: 0,
                    keyword_count: all_keywords.len(),
                    usable_keyword_count: usable.len(),
                    usable_keywords: usable,
                    is_over_limit: true,
                };
            }
        }

        KeywordUsage {
            tokens: usable_tokens,
            limit: KEYWORD_TOKEN_LIMIT,
            remaining_tokens: KEYWORD_TOKEN_LIMIT.saturating_sub(usable_tokens),
            keyword_count: all_keywords.len(),
            usable_keyword_count: usable.len(),
            usable_keywords: usable,
            is_over_limit: false,
        }
    }

    /// 2-Tier 디코딩에 사용할 한국어 초기 프롬프트를 생성합니다.
    pub fn build_prompts(&self, raw_keywords: &str) -> ConditionedPrompts {
        let usage = self.analyze_keywords(raw_keywords);
        let tier1_with_keywords = if usage.usable_keywords.is_empty() {
            None
        } else {
            Some(format!(
                " {} {}.",
                BASE_KO_PROMPT,
                usage.usable_keywords.join(", ")
            ))
        };

        ConditionedPrompts {
            tier1_with_keywords,
            tier2_base: BASE_KO_PROMPT.to_string(),
            usage,
        }
    }
}

/// `tokenizer.json` 다운로드 전에도 UI '30칸' 게이지가 자연스럽게 동작하도록 하는 Whisper GPT-2 바이트 BPE 근사치
fn estimate_whisper_bpe_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut ascii_run = 0usize;

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            ascii_run += 1;
            if ascii_run == 4 {
                tokens += 1;
                ascii_run = 0;
            }
        } else {
            if ascii_run > 0 {
                tokens += 1;
                ascii_run = 0;
            }
            if ch.is_whitespace() {
                continue;
            } else if ('\u{AC00}'..='\u{D7A3}').contains(&ch) {
                // 한국어 음절은 Whisper BPE에서 평균 1.3~1.8 토큰이지만 자주 쓰이는 음절 병합을 고려해 ~1.5 토큰으로 근사
                tokens += 2;
            } else {
                tokens += 1;
            }
        }
    }
    if ascii_run > 0 {
        tokens += 1;
    }
    // 한글 음절 과대 추정을 보정
    let hangul_count = text
        .chars()
        .filter(|&c| ('\u{AC00}'..='\u{D7A3}').contains(&c))
        .count();
    tokens.saturating_sub(hangul_count / 3).max(1)
}

/// 디코딩 결과에서 초기 프롬프트 문장이 그대로 환각으로 튀어나온 조각을 제거합니다.
pub fn strip_hint_fragments(raw_transcript: &str) -> String {
    let trimmed = raw_transcript.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 전체 문장이 기본 프롬프트 자체와 동일하면 빈 문자열 반환
    if trimmed == BASE_KO_PROMPT {
        return String::new();
    }

    let mut cleaned = trimmed.to_string();
    // 앞이나 뒤에 프롬프트 조각이 단독으로 에코된 경우 반복 제거
    let mut changed = true;
    while changed {
        changed = false;
        let cur = cleaned.trim();
        for frag in HINT_FRAGMENTS {
            if cur == frag {
                return String::new();
            }
            if let Some(rest) = cur.strip_prefix(frag) {
                cleaned = rest.trim().to_string();
                changed = true;
                break;
            }
        }
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_hint_fragments_removes_hallucinated_prompt() {
        assert_eq!(strip_hint_fragments(BASE_KO_PROMPT), "");
        assert_eq!(strip_hint_fragments("  안녕하세요.  "), "");
        assert_eq!(
            strip_hint_fragments("안녕하세요. 오늘 회의 자료 공유드립니다."),
            "오늘 회의 자료 공유드립니다."
        );
        assert_eq!(
            strip_hint_fragments("내일 오전 10시에 배포 진행할게요."),
            "내일 오전 10시에 배포 진행할게요."
        );
    }

    #[test]
    fn test_keyword_budget_enforces_token_limit() {
        let tok = PromptTokenizer::default();
        let short = tok.analyze_keywords("또박, 품의서, 클로드");
        assert!(!short.is_over_limit);
        assert_eq!(short.usable_keyword_count, 3);
        assert!(short.remaining_tokens < KEYWORD_TOKEN_LIMIT);

        let long_input = (0..30)
            .map(|i| format!("전문용어키워드{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        let over = tok.analyze_keywords(&long_input);
        assert!(over.is_over_limit);
        assert_eq!(over.remaining_tokens, 0);
        assert!(over.usable_keyword_count < over.keyword_count);
    }
}
