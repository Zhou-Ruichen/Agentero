//! jEV smart-highlight service.

use crate::core::error::AppError;
use crate::features::paper::analyze::parse::run_pdf_locate;
use crate::features::pdf::locate::LocateRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const JEV_MODEL: &str = "jev-latest";
const BATCH_SIZE: usize = 4;
const MIN_PARAGRAPH_LEN: usize = 80;
const MAX_PARAGRAPH_LEN: usize = 800;
const SCORE_THRESHOLD: f64 = 2.0;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedHighlight {
    pub quote: String,
    pub page: u32,
    pub rects: Vec<crate::features::pdf::locate::NormRect>,
    pub color: String,
    pub category: String,
    pub score: f64,
    pub page_width: f32,
    pub page_height: f32,
}

#[derive(Debug, Clone)]
struct Paragraph {
    id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct HighlightDecision {
    key: &'static str,
    color: &'static str,
    name: &'static str,
    score: f64,
}

const DIMS: [(&str, &str, &str); 5] = [
    ("method", "blue", "核心方法"),
    ("claim", "purple", "关键判断"),
    ("novelty", "green", "反常识结论"),
    ("effect", "yellow", "核心效果"),
    ("limitation", "pink", "局限"),
];

const COLOR_PRIORITY: [&str; 5] = ["method", "novelty", "effect", "claim", "limitation"];

/// Best-effort text extraction from a paper folder: PAPER.md > TeX source > PDF parse.
fn extract_paper_text(paper_dir: &Path) -> Vec<Paragraph> {
    // 1. Prefer PAPER.md
    let paper_md = paper_dir.join("PAPER.md");
    if paper_md.is_file() {
        if let Ok(text) = std::fs::read_to_string(&paper_md) {
            return split_paragraphs(&text);
        }
    }

    // 2. Fall back to TeX source
    let tex_candidates = ["source/main.tex", "source/main_zh-CN.tex", "main.tex"];
    for rel in &tex_candidates {
        let tex_path = paper_dir.join(rel);
        if tex_path.is_file() {
            if let Ok(text) = std::fs::read_to_string(&tex_path) {
                let plain = strip_latex(&text);
                return split_paragraphs(&plain);
            }
        }
    }

    Vec::new()
}

fn strip_latex(text: &str) -> String {
    // First pass: remove \begin{env}...\end{env} blocks
    let mut text = text.to_string();
    loop {
        let mut chars = text.chars().collect::<Vec<_>>();
        if let Some(start) = find_command(&chars, 0, "begin") {
            let after_begin = skip_command_tail(&chars, start);
            if after_begin < chars.len() && chars[after_begin] == '{' {
                let env_end = skip_bracket(&chars, after_begin, '{', '}');
                if env_end > after_begin + 1 {
                    let env_name: String = chars[after_begin + 1..env_end - 1].iter().collect();
                    let end_cmd = format!("\\end{{{}}}", env_name);
                    let end_chars: Vec<char> = end_cmd.chars().collect();
                    if let Some(end_pos) = find_subseq(&chars, env_end, &end_chars) {
                        chars.drain(start..end_pos + end_chars.len());
                        text = chars.into_iter().collect();
                        continue;
                    }
                }
            }
        }
        break;
    }

    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // Skip command name
            i += 1;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            // Skip optional star
            if i < chars.len() && chars[i] == '*' {
                i += 1;
            }
            // Skip optional [arg]
            if i < chars.len() && chars[i] == '[' {
                i = skip_bracket(&chars, i, '[', ']');
            }
            // Skip mandatory {arg}
            if i < chars.len() && chars[i] == '{' {
                i = skip_bracket(&chars, i, '{', '}');
            }
            out.push(' ');
            continue;
        }
        if c == '%' {
            // Skip comment to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '$' {
            // Inline or display math
            let mut end = i + 1;
            let display = end < chars.len() && chars[end] == '$';
            if display {
                end += 1;
            }
            while end < chars.len() {
                if display && chars[end] == '$' && end + 1 < chars.len() && chars[end + 1] == '$' {
                    end += 2;
                    break;
                }
                if !display && chars[end] == '$' {
                    end += 1;
                    break;
                }
                end += 1;
            }
            i = end;
            out.push(' ');
            continue;
        }
        if c == '{' || c == '}' {
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    collapse_whitespace(&out)
}

fn find_command(chars: &[char], start: usize, name: &str) -> Option<usize> {
    let name_chars: Vec<char> = name.chars().collect();
    for i in start..chars.len() {
        if chars[i] == '\\'
            && i + 1 + name_chars.len() <= chars.len()
            && chars[i + 1..i + 1 + name_chars.len()] == name_chars[..]
            && (i + 1 + name_chars.len() == chars.len()
                || !chars[i + 1 + name_chars.len()].is_alphabetic())
        {
            return Some(i);
        }
    }
    None
}

fn skip_command_tail(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len() && chars[i].is_alphabetic() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '*' {
        i += 1;
    }
    i
}

fn find_subseq(chars: &[char], start: usize, needle: &[char]) -> Option<usize> {
    if needle.is_empty() || start + needle.len() > chars.len() {
        return None;
    }
    for i in start..=chars.len() - needle.len() {
        if chars[i..i + needle.len()] == needle[..] {
            return Some(i);
        }
    }
    None
}

fn skip_bracket(chars: &[char], start: usize, open: char, close: char) -> usize {
    if start >= chars.len() || chars[start] != open {
        return start;
    }
    let mut depth = 1;
    let mut i = start + 1;
    while i < chars.len() && depth > 0 {
        if chars[i] == open {
            depth += 1;
        } else if chars[i] == close {
            depth -= 1;
        }
        i += 1;
    }
    i
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn split_paragraphs(text: &str) -> Vec<Paragraph> {
    let raw: Vec<&str> = text
        .split('\n')
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with("-") || trimmed.starts_with("*") {
                ""
            } else {
                line
            }
        })
        .collect();
    let joined = raw.join("\n");
    let blocks: Vec<&str> = joined.split('\n').collect();

    let mut paragraphs = Vec::new();
    let mut buf = String::new();
    for block in blocks {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            if !buf.trim().is_empty() {
                maybe_push_paragraph(&mut paragraphs, &buf);
                buf.clear();
            }
        } else {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(trimmed);
        }
    }
    if !buf.trim().is_empty() {
        maybe_push_paragraph(&mut paragraphs, &buf);
    }

    // Chunk long paragraphs at sentence boundaries
    let mut result = Vec::new();
    for p in paragraphs {
        if p.text.len() <= MAX_PARAGRAPH_LEN {
            result.push(p);
        } else {
            let sentences: Vec<&str> = p.text.split_inclusive(['.', '!', '?']).collect();
            let mut chunk = String::new();
            for s in sentences {
                if chunk.len() + s.len() > MAX_PARAGRAPH_LEN && !chunk.is_empty() {
                    maybe_push_paragraph(&mut result, &chunk);
                    chunk = s.to_string();
                } else {
                    chunk.push_str(s);
                }
            }
            if !chunk.trim().is_empty() {
                maybe_push_paragraph(&mut result, &chunk);
            }
        }
    }
    result
}

fn maybe_push_paragraph(out: &mut Vec<Paragraph>, text: &str) {
    let trimmed = text.trim();
    if trimmed.len() >= MIN_PARAGRAPH_LEN {
        let id = format!("p{}", out.len());
        out.push(Paragraph {
            id,
            text: trimmed.to_string(),
        });
    }
}

fn build_questions(paragraphs: &[Paragraph]) -> HashMap<String, Value> {
    let mut questions = HashMap::new();
    for p in paragraphs {
        for (key, _, _) in DIMS {
            let qid = format!("{}_{}_score", p.id, key);
            let (instructions, criteria) = match key {
                "method" => (
                    format!("Does `paragraphs['{}']` describe a key method, algorithm, or technical contribution of the paper?", p.id),
                    vec!["Not a method / only background", "Mentions a method but not central", "Key method or algorithm", "Novel core method that defines the paper"],
                ),
                "claim" => (
                    format!("Does `paragraphs['{}']` contain a key claim, judgment, or central argument of the paper?", p.id),
                    vec!["No claim / purely factual background", "Minor claim", "Key claim or judgment", "Central thesis of the paper"],
                ),
                "novelty" => (
                    format!("Does `paragraphs['{}']` state a conclusion or finding that differs from common practice or prior work?", p.id),
                    vec!["Aligns with common knowledge", "Slight twist", "Notably different from prior assumptions", "Directly contradicts or overturns conventional wisdom"],
                ),
                "effect" => (
                    format!("Does `paragraphs['{}']` present a core experimental result, effect, or main finding?", p.id),
                    vec!["No result / background", "Secondary result", "Important result or effect", "Main result that demonstrates the paper's value"],
                ),
                _ => (
                    format!("Does `paragraphs['{}']` discuss limitations, caveats, failure cases, or future work?", p.id),
                    vec!["No limitation mentioned", "Minor caveat", "Important limitation", "Critical limitation that affects interpretation"],
                ),
            };
            questions.insert(
                qid,
                serde_json::json!({
                    "type": "score",
                    "instructions": instructions,
                    "criteria": criteria,
                }),
            );
        }
    }
    questions
}

fn decide_highlight(scores: &HashMap<String, f64>) -> Option<HighlightDecision> {
    let mut candidates = Vec::new();
    for (key, color, name) in DIMS {
        if let Some(&score) = scores.get(key) {
            if score >= SCORE_THRESHOLD {
                candidates.push(HighlightDecision {
                    key,
                    color,
                    name,
                    score,
                });
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let priority_index: std::collections::HashMap<&str, usize> = COLOR_PRIORITY
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i))
        .collect();
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| priority_index[a.key].cmp(&priority_index[b.key]))
    });
    Some(candidates[0].clone())
}

async fn call_jev(api_key: &str, base_url: &str, request: Value) -> Result<Value, AppError> {
    if api_key.is_empty() {
        return Err(AppError::message("jEV API key is not configured"));
    }
    let client = reqwest::Client::new();
    let resp = client
        .post(base_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| AppError::message(format!("jEV request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "<could not read body>".to_string());
        return Err(AppError::message(format!(
            "jEV API error {}: {}",
            status, body
        )));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| AppError::message(format!("jEV response decode failed: {e}")))
}

/// Extract highlights for one paper by calling jEV and locating each chosen quote in the PDF.
pub async fn jev_suggest_highlights_for_paper(
    paper_dir: &Path,
    pdf_path: &Path,
    title: &str,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<SuggestedHighlight>, AppError> {
    let paragraphs = extract_paper_text(paper_dir);
    if paragraphs.is_empty() {
        return Err(AppError::message(
            "No readable text found for this paper (need PAPER.md, TeX source, or parseable PDF)",
        ));
    }

    let mut all_highlights: Vec<SuggestedHighlight> = Vec::new();

    for chunk in paragraphs.chunks(BATCH_SIZE) {
        let paras: HashMap<String, String> = chunk
            .iter()
            .map(|p| (p.id.clone(), p.text.clone()))
            .collect();
        let questions = build_questions(chunk);
        let request = serde_json::json!({
            "state": {
                "paper_title": title,
                "paragraphs": paras,
            },
            "model": JEV_MODEL,
            "questions": questions,
        });

        let resp = call_jev(api_key, base_url, request).await?;
        let answers = resp
            .get("answers")
            .and_then(|a| a.as_object())
            .cloned()
            .unwrap_or_default();

        for p in chunk {
            let mut scores = HashMap::new();
            for (key, _, _) in DIMS {
                let qid = format!("{}_{}_score", p.id, key);
                let score = answers
                    .get(&qid)
                    .and_then(|a| a.get("score"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                scores.insert(key.to_string(), score);
            }

            if let Some(decision) = decide_highlight(&scores) {
                let locate_result = run_pdf_locate(pdf_path, &LocateRequest::new(&p.text)).await;
                let best_match = locate_result
                    .ok()
                    .and_then(|r| r.matches.into_iter().next());
                if let Some(m) = best_match {
                    all_highlights.push(SuggestedHighlight {
                        quote: p.text.clone(),
                        page: m.page,
                        rects: m.rects,
                        color: decision.color.to_string(),
                        category: decision.name.to_string(),
                        score: decision.score,
                        page_width: m.page_width,
                        page_height: m.page_height,
                    });
                }
            }
        }
    }

    // Deduplicate by quote to avoid overlapping identical suggestions
    let mut seen = std::collections::HashSet::new();
    all_highlights.retain(|h| seen.insert(h.quote.clone()));

    Ok(all_highlights)
}
