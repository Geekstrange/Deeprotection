use crate::shell::DpShell;

pub fn builtin_colon(_args: &[String]) -> i32 {
    0
}

pub fn builtin_history(args: &[String], state: &DpShell) -> i32 {
    let entries = &state.history;
    let start = if let Some(n_str) = args.first() {
        match n_str.parse::<usize>() {
            Ok(n) => entries.len().saturating_sub(n),
            Err(_) => {
                eprintln!("dpshell: history: {}: numeric argument required", n_str);
                return 1;
            }
        }
    } else {
        0
    };

    for (i, cmd) in entries[start..].iter().enumerate() {
        println!("{:5}  {}", start + i + 1, cmd);
    }
    0
}
