use std::env;

pub fn builtin_exec(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Ok(());
    }

    use crate::parser::resolve_binary;
    let search_path = env::var("PATH").unwrap_or_default();
    let program = resolve_binary(&args[0], &search_path)
        .ok_or_else(|| format!("{}: command not found", args[0]))?;

    let c_program =
        std::ffi::CString::new(program.to_str().unwrap_or("")).map_err(|e| e.to_string())?;
    let c_argv: Vec<std::ffi::CString> = args
        .iter()
        .map(|s| std::ffi::CString::new(s.as_str()).unwrap())
        .collect();
    let env_pairs = crate::parser::sanitised_env();
    let c_env: Vec<std::ffi::CString> = env_pairs
        .iter()
        .map(|(k, v)| std::ffi::CString::new(format!("{}={}", k, v)).unwrap())
        .collect();

    nix::unistd::execve(&c_program, &c_argv, &c_env).map_err(|e| format!("{}: {}", args[0], e))?;
    Ok(())
}

pub fn builtin_eval(args: &[String]) -> String {
    args.join(" ")
}

pub fn read_source_file(path: &str) -> Result<Vec<String>, String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_string).collect())
        .map_err(|e| format!("{}: {}", path, e))
}
