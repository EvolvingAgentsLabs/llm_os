//! Tool-call parser — Rust port of
//! `C:/evolvingagents/skillos_mini/mobile/src/lib/llm/tool_parser.ts`
//! (which is in turn a port of `skillos/agent_runtime.py:865-1086`).
//!
//! SkillOS cartridges emit tool calls in five shapes that real bootstrap
//! models actually produce:
//!
//! - **A**.  `<tool_call name="x">{"a":1}</tool_call>`
//! - **A2**. `<tool_call name="x">{"a":1}` (unclosed — terminates at next tag/EOF)
//! - **B**.  `<tool_call>{"a":1}</tool_call>` (name inferred from arg keys)
//! - **C**.  `<tool_call>\nx\n{"a":1}\n</tool_call>` (name on first line)
//! - **D**.  ```` ```json\n[{"tool_name":"x","parameters":{…}}]\n``` ````
//!
//! The LLM-OS I/O daemon's `<|call|>` dispatcher trusts the GBNF grammar,
//! but always runs `parse_tool_calls` as a defense-in-depth fallback. See
//! refinement §2.4 + §4 R9.
//!
//! Pure functions; no I/O; no async; safe to call in a hot loop.

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One parsed tool invocation: a name and its raw JSON-ish argument string.
/// The caller is responsible for cleaning + parsing `args` further; we keep
/// it as a string here so the daemon can route it through repair logic
/// before final JSON validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub args: String,
}

/// Static aliases applied at the daemon dispatch layer.
/// `run_agent` was the legacy name; `delegate_to_agent` is canonical.
pub fn tool_aliases() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("run_agent", "delegate_to_agent");
        m
    })
}

// ────────────────────────────────────────────────────────────────────────
// Utilities
// ────────────────────────────────────────────────────────────────────────

/// Return the trimmed body of `<tag>…</tag>`, or `None`.
pub fn extract_tag_content(tag: &str, text: &str) -> Option<String> {
    // Pattern is per-call because `tag` is dynamic; small cost, safer than
    // unsafe global state. Daemon keeps a small LRU if this ever shows up
    // in profiling.
    let pat = format!(r"<{tag}>([\s\S]*?)</{tag}>", tag = regex::escape(tag));
    let re = Regex::new(&pat).ok()?;
    re.captures(text).map(|c| c[1].trim().to_string())
}

/// Return the first complete JSON object (or array) in `text`, ignoring
/// trailing garbage. Respects quoted strings and `\\` escapes — i.e. a `}`
/// inside a `"…"` does not close the outer object.
///
/// If no `{` (or `[`) is present, returns the input verbatim.
pub fn extract_json_object(text: &str) -> String {
    let bytes = text.as_bytes();
    let start = match bytes.iter().position(|&b| b == b'{' || b == b'[') {
        Some(i) => i,
        None => return text.to_string(),
    };
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if escape {
            escape = false;
            continue;
        }
        if c == b'\\' {
            escape = true;
            continue;
        }
        if c == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if c == b'{' || c == b'[' {
            depth += 1;
        } else if c == b'}' || c == b']' {
            depth -= 1;
            if depth == 0 {
                // Slice on byte boundaries; JSON syntax tokens are ASCII
                // so this is safe even when the surrounding text is UTF-8.
                return text[start..=i].to_string();
            }
        }
    }
    text.to_string()
}

/// Best-effort repair for JSON objects whose string values contain
/// unescaped `"` — Gemma's classic failure mode: a model writes
/// `{"path": "f", "content": "hello "world" end"}` and the parse fails
/// because `"world"` is treated as the value's closing quote.
///
/// The heuristic: a `"` is a *real* string terminator only if it is
/// followed by `,\s*"` (next key), `}` (end of object), or end-of-input.
/// Anything else is body text, even if literal `"`.
///
/// Returns `None` on input that doesn't look like a JSON object at all
/// or when no key/value pairs could be extracted.
pub fn repair_json_args(raw: &str) -> Option<HashMap<String, Value>> {
    let trimmed = raw.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    let mut inner = trimmed[1..trimmed.len() - 1].trim().to_string();
    let mut result: HashMap<String, Value> = HashMap::new();

    let key_re = Regex::new(r#"^"([^"]+)"\s*:\s*"#).ok()?;
    let next_key_after = Regex::new(r#"^,\s*""#).ok()?;
    let bareval_re = Regex::new(r"^([^,}]+)").ok()?;

    while !inner.is_empty() {
        let key_match = match key_re.captures(&inner) {
            Some(c) => c,
            None => break,
        };
        let key = key_match[1].to_string();
        let consumed = key_match.get(0).unwrap().end();
        inner = inner[consumed..].to_string();

        if inner.starts_with('"') {
            // String value — find the *real* closing quote.
            inner = inner[1..].to_string();
            let bytes = inner.as_bytes();
            let mut val_end: Option<usize> = None;
            for j in 0..bytes.len() {
                if bytes[j] != b'"' {
                    continue;
                }
                let after = &inner[j + 1..];
                if next_key_after.is_match(after)
                    || after.starts_with('}')
                    || after.is_empty()
                {
                    val_end = Some(j);
                    break;
                }
            }
            let end = match val_end {
                Some(e) => e,
                None => break,
            };
            result.insert(key, Value::String(inner[..end].to_string()));
            // Advance past the value + optional `,` and whitespace.
            let mut rest = inner[end + 1..].to_string();
            rest = rest
                .trim_start()
                .trim_start_matches(',')
                .trim_start()
                .to_string();
            inner = rest;
        } else if inner.starts_with('{') || inner.starts_with('[') {
            let nested = extract_json_object(&inner);
            let parsed: Value =
                serde_json::from_str(&nested).unwrap_or_else(|_| Value::String(nested.clone()));
            result.insert(key, parsed);
            let after = inner[nested.len()..].to_string();
            inner = after
                .trim_start()
                .trim_start_matches(',')
                .trim_start()
                .to_string();
        } else {
            // Bare value: number, bool, null, or unquoted token.
            let m = match bareval_re.captures(&inner) {
                Some(c) => c,
                None => break,
            };
            let raw_val = m[1].trim().to_string();
            let parsed: Value = serde_json::from_str(&raw_val)
                .unwrap_or_else(|_| Value::String(raw_val.clone()));
            result.insert(key, parsed);
            let consumed = m.get(0).unwrap().end();
            let after = inner[consumed..].to_string();
            inner = after
                .trim_start()
                .trim_start_matches(',')
                .trim_start()
                .to_string();
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Guess a tool name from the shape of its args object. Mirrors the
/// SkillOS heuristic table — order matters: `path+content` must be checked
/// before `path` alone, otherwise `write_file` would be misclassified as
/// `read_file`.
pub fn infer_tool_from_args(json_str: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(json_str).ok()?;
    let obj = parsed.as_object()?;
    let has = |k: &str| obj.contains_key(k);

    if has("agent_name") {
        return Some("delegate_to_agent".into());
    }
    if has("path") && has("content") {
        return Some("write_file".into());
    }
    if has("url") {
        return Some("web_fetch".into());
    }
    if has("prompt") {
        return Some("call_llm".into());
    }
    if has("pattern") {
        return Some("memory_search".into());
    }
    if has("type") && has("key") && has("value") {
        return Some("memory_store".into());
    }
    if has("type") && has("key") {
        return Some("memory_recall".into());
    }
    if has("path") {
        return Some("read_file".into());
    }
    if has("query") {
        return Some("call_llm".into());
    }
    None
}

// ────────────────────────────────────────────────────────────────────────
// Format D: ```json\n[ {tool_name, parameters} ]\n```
// ────────────────────────────────────────────────────────────────────────

fn parse_json_array_tools(response: &str) -> Vec<ToolCall> {
    let mut results = Vec::new();
    let fence_re = Regex::new(r"(?s)```json\s*\n(.*?)```").unwrap();
    let mut candidates: Vec<String> = fence_re
        .captures_iter(response)
        .map(|c| c[1].to_string())
        .collect();

    if candidates.is_empty() {
        let bare_re = Regex::new(r"(?s)(\[.*?\])").unwrap();
        candidates = bare_re
            .captures_iter(response)
            .map(|c| c[1].to_string())
            .collect();
    }

    for cand in candidates {
        let parsed: Value = match serde_json::from_str(cand.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let arr = match parsed.as_array() {
            Some(a) => a,
            None => continue,
        };
        for item in arr {
            let obj = match item.as_object() {
                Some(o) => o,
                None => continue,
            };
            let name_val = obj
                .get("tool_name")
                .or_else(|| obj.get("tool_call"))
                .or_else(|| obj.get("name"))
                .cloned()
                .unwrap_or(Value::Null);
            let params = obj
                .get("parameters")
                .or_else(|| obj.get("params"))
                .or_else(|| obj.get("arguments"))
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            let mut name = match name_val {
                Value::String(s) => s,
                _ => String::new(),
            };
            if name.is_empty() {
                let params_str = serde_json::to_string(&params).unwrap_or_default();
                if let Some(inferred) = infer_tool_from_args(&params_str) {
                    name = inferred;
                }
            }
            if !name.is_empty() {
                let args = serde_json::to_string(&params).unwrap_or_default();
                results.push(ToolCall { name, args });
            }
        }
    }
    results
}

// ────────────────────────────────────────────────────────────────────────
// Main entry
// ────────────────────────────────────────────────────────────────────────

/// Parse zero-or-more tool calls from a raw model response.
///
/// Tries shapes in order; first shape with any matches wins. (Matches the
/// TS reference behavior. A model that emits both A and D in the same
/// response is malformed; the daemon sees only A.)
pub fn parse_tool_calls(response: &str) -> Vec<ToolCall> {
    // Format A: <tool_call name="x">{…}</tool_call>
    let re_a = Regex::new(r#"(?s)<tool_call name="(.*?)">(.*?)</tool_call>"#).unwrap();
    let mut calls: Vec<ToolCall> = re_a
        .captures_iter(response)
        .map(|c| ToolCall {
            name: c[1].to_string(),
            args: c[2].to_string(),
        })
        .collect();
    if !calls.is_empty() {
        return calls;
    }

    // Format A2: unclosed — terminates at next <tool_call|<final_answer|EOF.
    let re_a2 =
        Regex::new(r#"(?s)<tool_call name="(.*?)">(.*?)(?=<tool_call|<final_answer|$)"#).unwrap();
    for m in re_a2.captures_iter(response) {
        let name = m[1].trim().to_string();
        let mut body = m[2].trim().to_string();
        // Trim a trailing close tag if present (rare in A2, common when
        // the regex eats up to the next token boundary).
        if let Some(stripped) = body.strip_suffix("</tool_call>") {
            body = stripped.trim().to_string();
        }
        if !name.is_empty() && !body.is_empty() {
            calls.push(ToolCall { name, args: body });
        }
    }
    if !calls.is_empty() {
        return calls;
    }

    // Format B/C: <tool_call>…</tool_call>
    let re_bc = Regex::new(r"(?s)<tool_call>(.*?)</tool_call>").unwrap();
    for m in re_bc.captures_iter(response) {
        let body = m[1].trim().to_string();
        if let Some(inferred) = infer_tool_from_args(&body) {
            calls.push(ToolCall {
                name: inferred,
                args: body,
            });
        } else if let Some(first_nl) = body.find('\n') {
            let first = body[..first_nl].trim().to_string();
            let rest = body[first_nl + 1..].trim().to_string();
            if !first.starts_with('{') && !rest.is_empty() {
                calls.push(ToolCall {
                    name: first,
                    args: rest,
                });
            }
        }
    }
    if !calls.is_empty() {
        return calls;
    }

    // Format D: JSON array (fenced or bare)
    parse_json_array_tools(response)
}

// ────────────────────────────────────────────────────────────────────────
// Tests — port of mobile/tests/tool_parser.spec.ts
// ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_object_returns_first_complete_object() {
        assert_eq!(
            extract_json_object(r#"{"a":1}<channel|>noise"#),
            r#"{"a":1}"#
        );
        assert_eq!(
            extract_json_object(r#"prefix {"a":{"b":"}"}} suffix"#),
            r#"{"a":{"b":"}"}}"#
        );
        assert_eq!(extract_json_object("no json here"), "no json here");
    }

    #[test]
    fn extract_json_object_respects_escapes_inside_strings() {
        assert_eq!(
            extract_json_object(r#"{"a":"\"}"}"#),
            r#"{"a":"\"}"}"#
        );
    }

    #[test]
    fn extract_tag_content_returns_trimmed_body() {
        assert_eq!(
            extract_tag_content("final_answer", "<final_answer>  done  </final_answer>"),
            Some("done".to_string())
        );
    }

    #[test]
    fn extract_tag_content_returns_none_for_missing_tag() {
        assert_eq!(extract_tag_content("x", "nothing"), None);
    }

    #[test]
    fn infer_tool_recognises_common_signatures() {
        assert_eq!(
            infer_tool_from_args(r#"{"agent_name":"x","task":"y"}"#).as_deref(),
            Some("delegate_to_agent")
        );
        assert_eq!(
            infer_tool_from_args(r#"{"path":"f","content":"x"}"#).as_deref(),
            Some("write_file")
        );
        assert_eq!(
            infer_tool_from_args(r#"{"url":"http://x"}"#).as_deref(),
            Some("web_fetch")
        );
        assert_eq!(
            infer_tool_from_args(r#"{"prompt":"hi"}"#).as_deref(),
            Some("call_llm")
        );
        assert_eq!(
            infer_tool_from_args(r#"{"path":"f"}"#).as_deref(),
            Some("read_file")
        );
        assert_eq!(
            infer_tool_from_args(r#"{"query":"x"}"#).as_deref(),
            Some("call_llm")
        );
    }

    #[test]
    fn infer_tool_returns_none_for_unknown_shape() {
        assert_eq!(infer_tool_from_args(r#"{"nope":1}"#), None);
        assert_eq!(infer_tool_from_args("not json"), None);
    }

    #[test]
    fn parse_format_a_named_tool_call() {
        let out = parse_tool_calls(
            r#"<tool_call name="write_file">{"path":"a","content":"x"}</tool_call>"#,
        );
        assert_eq!(
            out,
            vec![ToolCall {
                name: "write_file".into(),
                args: r#"{"path":"a","content":"x"}"#.into(),
            }]
        );
    }

    #[test]
    fn parse_format_a2_unclosed_terminates_at_next_tag() {
        let text = "<tool_call name=\"read_file\">{\"path\":\"a\"}\n<final_answer>done</final_answer>";
        let out = parse_tool_calls(text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "read_file");
        assert!(out[0].args.contains(r#""path":"a""#));
    }

    #[test]
    fn parse_format_b_bare_tool_call_infers_name() {
        let out =
            parse_tool_calls(r#"<tool_call>{"path":"a","content":"x"}</tool_call>"#);
        assert_eq!(
            out,
            vec![ToolCall {
                name: "write_file".into(),
                args: r#"{"path":"a","content":"x"}"#.into(),
            }]
        );
    }

    #[test]
    fn parse_format_c_bare_tool_call_with_name_on_first_line() {
        let out =
            parse_tool_calls("<tool_call>\nweb_fetch\n{\"url\":\"http://x\"}\n</tool_call>");
        assert_eq!(
            out,
            vec![ToolCall {
                name: "web_fetch".into(),
                args: r#"{"url":"http://x"}"#.into(),
            }]
        );
    }

    #[test]
    fn parse_format_d_json_array_in_fence() {
        let text = "```json\n[{\"tool_name\":\"write_file\",\"parameters\":{\"path\":\"p\",\"content\":\"c\"}}]\n```";
        let out = parse_tool_calls(text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "write_file");
        let parsed: Value = serde_json::from_str(&out[0].args).unwrap();
        assert_eq!(parsed["path"], "p");
        assert_eq!(parsed["content"], "c");
    }

    #[test]
    fn parse_format_d_bare_array_without_fence() {
        let text = r#"[{"name":"read_file","params":{"path":"p"}}]"#;
        let out = parse_tool_calls(text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "read_file");
    }

    #[test]
    fn parse_format_d_infers_name_from_parameters_when_absent() {
        let text = "```json\n[{\"parameters\":{\"url\":\"http://x\"}}]\n```";
        let out = parse_tool_calls(text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "web_fetch");
    }

    #[test]
    fn parse_multiple_calls_in_format_a() {
        let text = "<tool_call name=\"a\">{\"x\":1}</tool_call>\n<tool_call name=\"b\">{\"y\":2}</tool_call>";
        let out = parse_tool_calls(text);
        assert_eq!(
            out.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn parse_empty_response_returns_no_calls() {
        assert_eq!(parse_tool_calls(""), vec![]);
        assert_eq!(parse_tool_calls("hello"), vec![]);
    }

    #[test]
    fn tool_aliases_map_run_agent_to_delegate_to_agent() {
        assert_eq!(*tool_aliases().get("run_agent").unwrap(), "delegate_to_agent");
    }

    #[test]
    fn repair_recovers_args_with_unescaped_inner_quotes() {
        // Gemma's classic failure: `content` has unescaped quotes but the
        // outer `"," boundary is well-formed.
        let raw = r#"{"path": "f.md", "content": "hello "world" end"}"#;
        let out = repair_json_args(raw).expect("repair should succeed");
        assert_eq!(out.get("path"), Some(&Value::String("f.md".into())));
        let content = out.get("content").unwrap().as_str().unwrap();
        assert!(content.contains(r#""world""#));
        assert!(content.contains("hello"));
        assert!(content.contains("end"));
    }

    #[test]
    fn repair_returns_none_for_non_object_input() {
        assert!(repair_json_args("nope").is_none());
        assert!(repair_json_args("[1,2]").is_none());
    }
}
