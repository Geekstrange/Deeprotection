// rules.rs - ARCHITECTURE.md §3.2 Rule Matching Module
use crate::config::Rule;
use regex::Regex;

// ANSI color codes for user-facing messages
const RED_BLINK: &str = "\x1b[31;5m";
const YELLOW_BLINK: &str = "\x1b[33;5m";
const RESET: &str = "\x1b[0m";

/// A compiled, ready-to-match rule.
pub struct CompiledRule {
    pub name: String,
    pub regex: Regex,
    pub action: RuleAction,
}

/// Resolved action variant.
pub enum RuleAction {
    Block,
    Replace(String),
}

/// Convert a plain-string pattern to a regex that allows surrounding whitespace.
/// ARCHITECTURE.md §3.2: "Simple string → auto regex"
/// Refactored_Plan.md §1: plain strings auto-converted to `^\s*<escaped>\s*$`
/// with internal spaces converted to `\s+` for flexible matching.
pub fn simple_to_regex(pattern: &str) -> String {
    let escaped = regex::escape(pattern);
    // Replace escaped spaces with \s+ so "rm -rf" matches "rm  -rf" etc.
    let with_whitespace = escaped.replace(r"\ ", r"\s+");
    format!(r"^\s*{}\s*$", with_whitespace)
}

/// Compile a single Rule into a CompiledRule.
/// Returns None if the rule is disabled or the regex is invalid.
pub fn compile_rule(rule: &Rule) -> Option<CompiledRule> {
    if !rule.enabled {
        return None;
    }

    let regex = if let Some(re_str) = rule.pattern.strip_prefix("re:") {
        // Explicit regex: prefix "re:" signals a raw regex pattern
        Regex::new(re_str).ok()?
    } else {
        // Plain string: auto-convert to anchored regex
        let regex_str = simple_to_regex(&rule.pattern);
        Regex::new(&regex_str).ok()?
    };

    let action = if rule.action.is_block() {
        RuleAction::Block
    } else if let Some(replacement) = rule.action.replacement() {
        RuleAction::Replace(replacement.to_string())
    } else {
        // Malformed rule (neither block nor replace) — skip
        return None;
    };

    Some(CompiledRule {
        name: rule.name.clone(),
        regex,
        action,
    })
}

/// Apply rules in order; first match wins (ARCHITECTURE.md §3.2).
///
/// For `Replace` actions, any content in the original command that follows
/// the matched portion is preserved and appended to the replacement string.
/// This means flags/arguments explicitly covered by the pattern are dropped
/// (intentional — that is the rule author's intent), while any trailing
/// operands (filenames, paths, etc.) are carried over.
///
/// Examples with pattern `re:rm\s*` and replacement `rm -iv`:
///   `rm abc`    → matched `rm `, suffix `abc`   → `rm -iv abc`
///   `rm -f abc` → matched `rm -f`, suffix `abc` → `rm -iv abc`
///   `rm`        → matched `rm`,   suffix ``     → `rm -iv`
///
/// Returns:
/// - `Some(cmd)` — either the original command (no match) or the final
///                 replacement (with suffix appended when non-empty)
/// - `None`      — command was blocked
pub fn apply_rules(command: &str, rules: &[CompiledRule]) -> Option<String> {
    for rule in rules {
        // Use `find` instead of `is_match` so we can extract the match range.
        if let Some(m) = rule.regex.find(command) {
            match &rule.action {
                RuleAction::Block => {
                    println!("{RED_BLINK}[!]{RESET} Blocked by rule: {}", rule.name);
                    return None; // Signal: block
                }
                RuleAction::Replace(new_cmd) => {
                    // Everything after the matched portion, with surrounding
                    // whitespace stripped.  For a fully-anchored plain-string
                    // rule this will always be empty; for a partial `re:` pattern
                    // it captures the trailing operands the author didn't mention.
                    let suffix = command[m.end()..].trim();

                    let final_cmd = if suffix.is_empty() {
                        new_cmd.clone()
                    } else {
                        format!("{} {}", new_cmd, suffix)
                    };

                    println!(
                        "{YELLOW_BLINK}<!>{RESET} Replaced by rule '{}': {} → {}",
                        rule.name, command, final_cmd
                    );
                    return Some(final_cmd);
                }
            }
        }
    }
    // No rule matched — return original command unchanged
    Some(command.to_string())
}