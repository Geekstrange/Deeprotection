use std::os::unix::fs::PermissionsExt;

pub fn builtin_test(args: &[String]) -> i32 {
    let args: Vec<&str> = {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        if a.first() == Some(&"[") && a.last() == Some(&"]") {
            a[1..a.len() - 1].to_vec()
        } else {
            a
        }
    };

    match eval_test_expr(&args) {
        true => 0,
        false => 1,
    }
}

fn eval_test_expr(args: &[&str]) -> bool {
    match args {
        [] => false,
        ["!", rest @ ..] => !eval_test_expr(rest),
        [a, "-a", b @ ..] => eval_test_expr(&[a]) && eval_test_expr(b),
        [a, "-o", b @ ..] => eval_test_expr(&[a]) || eval_test_expr(b),

        ["-e", f] => std::path::Path::new(f).exists(),
        ["-f", f] => std::path::Path::new(f).is_file(),
        ["-d", f] => std::path::Path::new(f).is_dir(),
        ["-L", f] | ["-h", f] => std::fs::symlink_metadata(f)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        ["-r", f] => file_test_mode(f, 0o444),
        ["-w", f] => file_test_mode(f, 0o222),
        ["-x", f] => file_test_mode(f, 0o111),
        ["-s", f] => std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false),

        ["-z", s] => s.is_empty(),
        ["-n", s] => !s.is_empty(),
        [a, "=", b] => a == b,
        [a, "!=", b] => a != b,
        [a, "<", b] => a < b,
        [a, ">", b] => a > b,

        [a, op, b] => {
            if let (Ok(n1), Ok(n2)) = (a.parse::<i64>(), b.parse::<i64>()) {
                match *op {
                    "-eq" => n1 == n2,
                    "-ne" => n1 != n2,
                    "-lt" => n1 < n2,
                    "-le" => n1 <= n2,
                    "-gt" => n1 > n2,
                    "-ge" => n1 >= n2,
                    _ => {
                        eprintln!("dpshell: test: {}: unknown operator", op);
                        false
                    }
                }
            } else {
                eprintln!("dpshell: test: integer expression expected");
                false
            }
        }

        [s] => !s.is_empty(),

        _ => {
            eprintln!("dpshell: test: too many arguments");
            false
        }
    }
}

fn file_test_mode(path: &str, mode: u32) -> bool {
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & mode != 0)
        .unwrap_or(false)
}
