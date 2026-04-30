// rules.rs - ARCHITECTURE.md §3.2 Rule Matching Module (Phase 3)
//
// SECURITY-CRITICAL CHANGES vs. original:
//
//   1. simple_to_regex() actually applies whitespace flexibility now.  The
//      previous implementation called regex::escape() and then tried to
//      replace `\ ` (backslash-space) with `\s+` — but regex::escape() does
//      NOT escape spaces, so the replace was a no-op.  Result: a rule
//      `pattern = "rm -rf"` only matched ONE space; users could trivially
//      bypass with `rm  -rf` (two spaces).
//
//   2. compile_rule() now reports invalid rules to stderr instead of silently
//      dropping them via filter_map.

use crate::config::Rule;
use crate::syntax::{CommandNode, SimpleCommand};
use regex::Regex;

const RED_BLINK:    &str = "\x1b[31;5m";
const YELLOW_BLINK: &str = "\x1b[33;5m";
const RESET:        &str = "\x1b[0m";

pub struct CompiledRule {
    pub name:    String,
    pub matcher: RuleMatcher,
    pub action:  RuleAction,
}

pub enum RuleMatcher {
    RawRegex(Regex),
    CommandName(String),
    AnyArg(Regex),
}

pub enum RuleAction {
    Block,
    Replace(String),
}

/// Convert a "plain" pattern like `rm -rf` into an anchored regex that:
///   • collapses all internal whitespace runs to `\s+` (so multiple spaces
///     match a single user-supplied space and vice-versa);
///   • allows leading/trailing whitespace via `^\s* … \s*$`.
pub fn simple_to_regex(pattern: &str) -> String {
    // ── BUG FIX ──
    // Old code: `regex::escape(p).replace(r"\ ", r"\s+")` — but regex::escape
    // does not produce a backslash-space sequence, so the replace was a no-op
    // and only literal single-space matched.  Now we split-on-whitespace,
    // escape each part, then re-join with `\s+`.
    let parts: Vec<String> = pattern
        .split_whitespace()
        .map(regex::escape)
        .collect();
    let joined = parts.join(r"\s+");
    format!(r"^\s*{}\s*$", joined)
}

pub fn compile_rule(rule: &Rule) -> Option<CompiledRule> {
    if !rule.enabled {
        return None;
    }

    let matcher = if let Some(re_str) = rule.pattern.strip_prefix("re:") {
        match Regex::new(re_str) {
            Ok(r) => RuleMatcher::RawRegex(r),
            Err(e) => {
                eprintln!("dpshell: rule '{}': invalid regex '{}': {}", rule.name, re_str, e);
                return None;
            }
        }
    } else if let Some(name) = rule.pattern.strip_prefix("cmd:") {
        RuleMatcher::CommandName(name.to_string())
    } else if let Some(re_str) = rule.pattern.strip_prefix("arg:") {
        match Regex::new(re_str) {
            Ok(r) => RuleMatcher::AnyArg(r),
            Err(e) => {
                eprintln!("dpshell: rule '{}': invalid arg regex '{}': {}", rule.name, re_str, e);
                return None;
            }
        }
    } else {
        let re_str = simple_to_regex(&rule.pattern);
        match Regex::new(&re_str) {
            Ok(r) => RuleMatcher::RawRegex(r),
            Err(e) => {
                eprintln!("dpshell: rule '{}': internal regex compile error: {}", rule.name, e);
                return None;
            }
        }
    };

    let action = if rule.action.is_block() {
        RuleAction::Block
    } else if let Some(r) = rule.action.replacement() {
        RuleAction::Replace(r.to_string())
    } else {
        eprintln!(
            "dpshell: rule '{}': has neither block nor replace action — skipping",
            rule.name
        );
        return None;
    };

    Some(CompiledRule { name: rule.name.clone(), matcher, action })
}

fn try_match_simple(rule: &CompiledRule, sc: &SimpleCommand) -> Option<String> {
    match &rule.matcher {
        RuleMatcher::RawRegex(re) => {
            let m = re.find(&sc.raw)?;
            Some(sc.raw[m.end()..].trim().to_string())
        }
        RuleMatcher::CommandName(name) => {
            if sc.name() == name { Some(sc.args().join(" ")) } else { None }
        }
        RuleMatcher::AnyArg(re) => {
            if sc.args().iter().any(|a| re.is_match(a)) {
                Some(sc.args().join(" "))
            } else {
                None
            }
        }
    }
}

fn apply_rules_to_simple(
    sc: &SimpleCommand,
    rules: &[CompiledRule],
) -> Result<Option<String>, ()> {
    for rule in rules {
        if let Some(suffix) = try_match_simple(rule, sc) {
            match &rule.action {
                RuleAction::Block => {
                    // Use stderr for security-relevant output (was println!).
                    eprintln!("{RED_BLINK}[!]{RESET} Blocked by rule '{}': {}", rule.name, sc.raw);
                    return Err(());
                }
                RuleAction::Replace(new_cmd) => {
                    let final_cmd = if suffix.is_empty() {
                        new_cmd.clone()
                    } else {
                        format!("{} {}", new_cmd, suffix)
                    };
                    eprintln!("{YELLOW_BLINK}<!>{RESET} Replaced by rule '{}': {} → {}",
                        rule.name, sc.raw, final_cmd);
                    return Ok(Some(final_cmd));
                }
            }
        }
    }
    Ok(None)
}

pub fn apply_rules_to_node(
    node: CommandNode,
    rules: &[CompiledRule],
) -> Option<CommandNode> {
    match node {
        CommandNode::Simple(sc) => {
            match apply_rules_to_simple(&sc, rules) {
                Err(()) => None,
                Ok(None) => Some(CommandNode::Simple(sc)),
                Ok(Some(replacement)) => {
                    match crate::syntax::parse_command_line(&replacement) {
                        Ok(new_node) => Some(new_node),
                        Err(e) => {
                            eprintln!("dpshell: rule replacement parse error: {}", e);
                            None
                        }
                    }
                }
            }
        }

        CommandNode::Pipeline(cmds) => {
            let mut new_cmds: Vec<crate::syntax::SimpleCommand> = Vec::new();
            for sc in cmds {
                match apply_rules_to_simple(&sc, rules) {
                    Err(()) => return None,
                    Ok(None) => new_cmds.push(sc),
                    Ok(Some(replacement)) => {
                        match crate::syntax::parse_command_line(&replacement) {
                            Ok(CommandNode::Simple(new_sc)) => new_cmds.push(new_sc),
                            Ok(_) => {
                                eprintln!("dpshell: rule cannot replace a pipeline stage \
                                           with a compound expression");
                                return None;
                            }
                            Err(e) => {
                                eprintln!("dpshell: rule replacement parse error: {}", e);
                                return None;
                            }
                        }
                    }
                }
            }
            Some(CommandNode::Pipeline(new_cmds))
        }

        CommandNode::Logical { left, op, right } => {
            let new_left  = apply_rules_to_node(*left,  rules)?;
            let new_right = apply_rules_to_node(*right, rules)?;
            Some(CommandNode::Logical {
                left:  Box::new(new_left),
                op,
                right: Box::new(new_right),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_flexibility_works() {
        let re_str = simple_to_regex("rm -rf");
        let re = Regex::new(&re_str).unwrap();
        // Single space (the original case): must still match.
        assert!(re.is_match("rm -rf"));
        // Multiple spaces (regression): must also match.
        assert!(re.is_match("rm  -rf"));
        assert!(re.is_match("rm   -rf"));
        // Tab: must also match (it is whitespace).
        assert!(re.is_match("rm\t-rf"));
        // Leading/trailing whitespace: must match.
        assert!(re.is_match("  rm -rf  "));
        // Different command: must not match.
        assert!(!re.is_match("rmdir -rf"));
    }
}
