use crate::shell::DpShell;
use std::collections::HashMap;
use std::env;
use std::sync::Mutex;

static PATH_CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, String>) -> R,
{
    let mut guard = PATH_CACHE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

pub fn builtin_hash(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        return with_cache(|c| {
            if c.is_empty() {
                eprintln!("hash: hash table empty");
                return 0;
            }
            for (name, path) in c.iter() {
                println!("{}\t{}", name, path);
            }
            0
        });
    }

    if args.first().map(String::as_str) == Some("-r") {
        with_cache(|c| c.clear());
        return 0;
    }

    let search_path = env::var("PATH").unwrap_or_default();
    let mut rc = 0;
    for name in args {
        if name.starts_with('-') {
            continue;
        }
        match crate::parser::resolve_binary(name, &search_path) {
            Some(p) => {
                let path_str = p.to_string_lossy().to_string();
                with_cache(|c| c.insert(name.clone(), path_str));
            }
            None => {
                eprintln!("dpshell: hash: {}: not found", name);
                rc = 1;
            }
        }
    }
    rc
}
