use crate::shell::DpShell;

pub fn builtin_bind(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        return 0;
    }
    for arg in args {
        match arg.as_str() {
            "-p" | "-P" => {
                println!("# dpshell: key bindings listing not yet supported");
            }
            "-l" => {
                println!("# dpshell: readline function names listing not yet supported");
            }
            "-v" | "-V" => {
                println!("# dpshell: readline variable listing not yet supported");
            }
            _ => {}
        }
    }
    0
}
