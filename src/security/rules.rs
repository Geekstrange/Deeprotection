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
use crate::parser::syntax::{CommandNode, SimpleCommand};
use regex::Regex;

const RED_BLINK: &str = "\x1b[31;5m";
const YELLOW_BLINK: &str = "\x1b[33;5m";
const RESET: &str = "\x1b[0m";

pub struct CompiledRule {
    pub name: String,
    pub matcher: RuleMatcher,
    pub action: RuleAction,
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
    let parts: Vec<String> = pattern.split_whitespace().map(regex::escape).collect();
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
                eprintln!(
                    "dpshell: rule '{}': invalid regex '{}': {}",
                    rule.name, re_str, e
                );
                return None;
            }
        }
    } else if let Some(name) = rule.pattern.strip_prefix("cmd:") {
        RuleMatcher::CommandName(name.to_string())
    } else if let Some(re_str) = rule.pattern.strip_prefix("arg:") {
        match Regex::new(re_str) {
            Ok(r) => RuleMatcher::AnyArg(r),
            Err(e) => {
                eprintln!(
                    "dpshell: rule '{}': invalid arg regex '{}': {}",
                    rule.name, re_str, e
                );
                return None;
            }
        }
    } else {
        let re_str = simple_to_regex(&rule.pattern);
        match Regex::new(&re_str) {
            Ok(r) => RuleMatcher::RawRegex(r),
            Err(e) => {
                eprintln!(
                    "dpshell: rule '{}': internal regex compile error: {}",
                    rule.name, e
                );
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

    Some(CompiledRule {
        name: rule.name.clone(),
        matcher,
        action,
    })
}

fn try_match_simple(rule: &CompiledRule, sc: &SimpleCommand) -> Option<String> {
    match &rule.matcher {
        RuleMatcher::RawRegex(re) => {
            let m = re.find(&sc.raw)?;
            Some(sc.raw[m.end()..].trim().to_string())
        }
        RuleMatcher::CommandName(name) => {
            if sc.name() == name {
                Some(sc.args().join(" "))
            } else {
                None
            }
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

fn apply_rules_to_simple(sc: &SimpleCommand, rules: &[CompiledRule]) -> Result<Option<String>, ()> {
    for rule in rules {
        if let Some(suffix) = try_match_simple(rule, sc) {
            match &rule.action {
                RuleAction::Block => {
                    // Use stderr for security-relevant output (was println!).
                    eprintln!(
                        "{RED_BLINK}[!]{RESET} Blocked by rule '{}': {}",
                        rule.name, sc.raw
                    );
                    return Err(());
                }
                RuleAction::Replace(new_cmd) => {
                    let final_cmd = if suffix.is_empty() {
                        new_cmd.clone()
                    } else {
                        format!("{} {}", new_cmd, suffix)
                    };
                    eprintln!(
                        "{YELLOW_BLINK}<!>{RESET} Replaced by rule '{}': {} → {}",
                        rule.name, sc.raw, final_cmd
                    );
                    return Ok(Some(final_cmd));
                }
            }
        }
    }
    Ok(None)
}

pub fn apply_rules_to_node(node: CommandNode, rules: &[CompiledRule]) -> Option<CommandNode> {
    match node {
        CommandNode::Simple(sc) => match apply_rules_to_simple(&sc, rules) {
            Err(()) => None,
            Ok(None) => Some(CommandNode::Simple(sc)),
            Ok(Some(replacement)) => {
                match crate::parser::syntax::parse_command_line(&replacement) {
                    Ok(new_node) => Some(new_node),
                    Err(e) => {
                        eprintln!("dpshell: rule replacement parse error: {}", e);
                        None
                    }
                }
            }
        },

        CommandNode::Pipeline(cmds) => {
            let mut new_cmds: Vec<crate::parser::syntax::SimpleCommand> = Vec::new();
            for sc in cmds {
                match apply_rules_to_simple(&sc, rules) {
                    Err(()) => return None,
                    Ok(None) => new_cmds.push(sc),
                    Ok(Some(replacement)) => {
                        match crate::parser::syntax::parse_command_line(&replacement) {
                            Ok(CommandNode::Simple(new_sc)) => new_cmds.push(new_sc),
                            Ok(_) => {
                                eprintln!(
                                    "dpshell: rule cannot replace a pipeline stage \
                                           with a compound expression"
                                );
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
            let new_left = apply_rules_to_node(*left, rules)?;
            let new_right = apply_rules_to_node(*right, rules)?;
            Some(CommandNode::Logical {
                left: Box::new(new_left),
                op,
                right: Box::new(new_right),
            })
        }
        CommandNode::Background(inner) => {
            let new_inner = apply_rules_to_node(*inner, rules)?;
            Some(CommandNode::Background(Box::new(new_inner)))
        }
        CommandNode::Compound(nodes) => {
            let mut new_nodes = Vec::new();
            for n in nodes {
                new_nodes.push(apply_rules_to_node(n, rules)?);
            }
            Some(CommandNode::Compound(new_nodes))
        }
        CommandNode::FunctionDef { name, body } => {
            let new_body = apply_rules_to_node(*body, rules)?;
            Some(CommandNode::FunctionDef {
                name,
                body: Box::new(new_body),
            })
        }
        CommandNode::If { cond, then_body, elifs, else_body } => {
            let new_cond = apply_rules_to_node(*cond, rules)?;
            let new_then: Vec<CommandNode> =
                then_body.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
            let new_elifs: Vec<(CommandNode, Vec<CommandNode>)> = elifs.into_iter()
                .filter_map(|(c, b)| {
                    let nc = apply_rules_to_node(c, rules)?;
                    let nb = b.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
                    Some((nc, nb))
                })
                .collect();
            let new_else: Vec<CommandNode> =
                else_body.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
            Some(CommandNode::If { cond: Box::new(new_cond), then_body: new_then, elifs: new_elifs, else_body: new_else })
        }
        CommandNode::For { var, words, body } => {
            let new_body: Vec<CommandNode> =
                body.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
            Some(CommandNode::For { var, words, body: new_body })
        }
        CommandNode::While { cond, body, redirections } => {
            let new_cond = apply_rules_to_node(*cond, rules)?;
            let new_body: Vec<CommandNode> =
                body.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
            Some(CommandNode::While { cond: Box::new(new_cond), body: new_body, redirections })
        }
        CommandNode::Until { cond, body, redirections } => {
            let new_cond = apply_rules_to_node(*cond, rules)?;
            let new_body: Vec<CommandNode> =
                body.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
            Some(CommandNode::Until { cond: Box::new(new_cond), body: new_body, redirections })
        }
        CommandNode::Case { word, arms } => {
            let new_arms: Vec<(Vec<String>, Vec<CommandNode>)> = arms.into_iter()
                .map(|(pats, b)| {
                    let nb = b.into_iter().filter_map(|n| apply_rules_to_node(n, rules)).collect();
                    (pats, nb)
                })
                .collect();
            Some(CommandNode::Case { word, arms: new_arms })
        }
    }
}

/// Find byte offsets immediately after each unquoted `&&`, `||`, or `;`
/// operator in the raw input.  These offsets are used to create "virtual
/// line starts" so that `^`-anchored regexes can match patterns embedded
/// after compound operators.
fn compound_tail_offsets(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut offsets: Vec<usize> = Vec::new();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < len && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < len { i += 1; }
            }
            b'"' => {
                i += 1;
                while i < len && bytes[i] != b'"' {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
                if i < len { i += 1; }
            }
            b'\\' => {
                i += 2;
            }
            b'&' if i + 1 < len && bytes[i + 1] == b'&' => {
                i += 2;
                offsets.push(i);
            }
            b'|' if i + 1 < len && bytes[i + 1] == b'|' => {
                i += 2;
                offsets.push(i);
            }
            b';' => {
                i += 1;
                offsets.push(i);
            }
            _ => { i += 1; }
        }
    }
    offsets
}

/// Pre-AST raw-input rule check.  Tests `RawRegex` rules against the full
/// input line AND each tail substring after unquoted `&&`, `||`, `;`
/// operators.  This catches structural patterns (e.g. fork bombs) even when
/// embedded inside compound commands like `echo hi && :(){:|:&};:`.
///
/// Returns `None` if a blocking rule matched (command should be rejected).
/// Returns `Some(())` if all rules pass.
pub fn check_raw_input(raw_input: &str, rules: &[CompiledRule]) -> Option<()> {
    let tails = compound_tail_offsets(raw_input);

    for rule in rules {
        if let RuleMatcher::RawRegex(ref re) = rule.matcher {
            if let RuleAction::Block = rule.action {
                if re.is_match(raw_input) {
                    eprintln!(
                        "{RED_BLINK}[!]{RESET} Blocked by rule '{}': {}",
                        rule.name, raw_input
                    );
                    return None;
                }
                for &offset in &tails {
                    let tail = &raw_input[offset..];
                    if !tail.trim().is_empty() && re.is_match(tail.trim_start()) {
                        eprintln!(
                            "{RED_BLINK}[!]{RESET} Blocked by rule '{}': {}",
                            rule.name, raw_input
                        );
                        return None;
                    }
                }
            }
        }
    }
    Some(())
}
