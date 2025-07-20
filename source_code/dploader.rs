use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
//use futures::StreamExt;
use serde_json::Value;
use tokio::signal;

const CONFIG_FILE: &str = "/etc/deeprotection/deeprotection.conf";
const LANG_DIR: &str = "/usr/share/locale/deeprotection";
const REPO_OWNER: &str = "Geekstrange";
const REPO_NAME: &str = "Deeprotection";
const DOWNLOAD_DIR: &str = "/tmp";
const CHUNK_SIZE: u64 = 1024 * 1024;

// Color constants
const RED_WD: &str = "\x1b[31m";
const GREEN_WD: &str = "\x1b[32m";
const BLINK: &str = "\x1b[5m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

#[derive(Debug, Clone)]
struct Messages {
    map: HashMap<String, String>,
}

impl Messages {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    fn get(&self, key: &str) -> String {
        self.map.get(key).cloned().unwrap_or_else(|| key.to_string())
    }
}

#[derive(Debug)]
struct AppContext {
    messages: Messages,
}

impl AppContext {
    async fn new() -> Result<Self> {
        let lang_code = get_lang_code()?;
        let messages = load_language(&lang_code).await?;
        Ok(Self { messages })
    }
}

fn parse_ftl(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }
        let key = parts[0].trim().replace(' ', "");
        let mut value = parts[1].trim().to_string();
        if value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1].to_string();
        }
        value = value.replace("\\\"", "\"").replace("\\\\", "\\").replace("\\n", "\n");
        map.insert(key, value);
    }
    map
}

async fn load_language(lang_code: &str) -> Result<Messages> {
    let ftl_file = format!("{}/{}.ftl", LANG_DIR, lang_code);
    let default_file = format!("{}/en_US.ftl", LANG_DIR);
    let mut messages = Messages::new();

    if Path::new(&ftl_file).exists() {
        let content = fs::read_to_string(&ftl_file)?;
        messages.map = parse_ftl(&content);
        return Ok(messages);
    }

    if Path::new(&default_file).exists() {
        let content = fs::read_to_string(&default_file)?;
        messages.map = parse_ftl(&content);
        println!("{}Using default language.{}", GREEN_WD, RESET);
        return Ok(messages);
    }

    println!("{}Error: No language files found{}", RED_WD, RESET);
    Err("".into())
}

fn get_lang_code() -> Result<String> {
    if let Ok(content) = fs::read_to_string(CONFIG_FILE) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("language") && line.contains('=') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let mut value = parts[1].trim();
                    if let Some(comment_pos) = value.find('#') {
                        value = &value[..comment_pos];
                    }
                    let value = value.trim().replace(' ', "").replace('\t', "");
                    if !value.is_empty() {
                        return Ok(value);
                    }
                }
            }
        }
    }

    let system_lang = env::var("LC_ALL")
        .or_else(|_| env::var("LANG"))
        .unwrap_or_else(|_| "en_US".to_string());
    let lang_code = system_lang.split('.').next().unwrap_or("en_US").to_string();
    let system_ftl = format!("{}/{}.ftl", LANG_DIR, lang_code);
    if Path::new(&system_ftl).exists() {
        Ok(lang_code)
    } else {
        Ok("en_US".to_string())
    }
}

fn call_dp() -> Result<()> {
    let output = Command::new("which").arg("dp").output()?;
    if output.status.success() {
        let dp_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Command::new(dp_path).status()?;
    }
    Ok(())
}

async fn start_download(deb_asset: &str, messages: &Messages) -> Result<String> {
    let filename = deb_asset.split('/').last().unwrap_or("package.deb");
    let download_path = format!("{}/{}", DOWNLOAD_DIR, filename);

    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = interrupted.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        interrupted_clone.store(true, Ordering::SeqCst);
    });

    print!("{}...", messages.get("msg_is_downloading"));
    io::stdout().flush()?;

    let client = reqwest::Client::builder()
        .user_agent("Deeprotection-Updater/1.0")
        .http2_prior_knowledge()
        .build()?;

    let head_resp = client.head(deb_asset).send().await?;
    let total_size = head_resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    if total_size == 0 {
        return Err("Cannot get file size".into());
    }

    let mut file = File::create(&download_path).await?;
    let chunk_count = (total_size + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let downloaded = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];
    for i in 0..chunk_count {
        let client = client.clone();
        let url = deb_asset.to_string();
        let downloaded = downloaded.clone();
        let interrupted = interrupted.clone();

        handles.push(tokio::spawn(async move {
            let start = i * CHUNK_SIZE;
            let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
            let range = format!("bytes={}-{}", start, end);

            let mut resp = client
                .get(&url)
                .header("Range", range)
                .send()
                .await?
                .error_for_status()?;

            let mut buffer = Vec::with_capacity((end - start + 1) as usize);
            while let Some(chunk) = resp.chunk().await? {
                if interrupted.load(Ordering::SeqCst) {
                    return Err("User interrupted".into());
                }
                buffer.extend_from_slice(&chunk);
            }

            downloaded.fetch_add(buffer.len(), Ordering::SeqCst);
            Ok::<(u64, Vec<u8>), Box<dyn std::error::Error + Send + Sync>>((start, buffer))
        }));
    }

    for handle in handles {
        let (start, data) = handle.await??;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        file.write_all(&data).await?;
    }

    if interrupted.load(Ordering::SeqCst) {
        print!("\r\x1b[K{}{}[!]{} {}{}{}", RED_WD, BLINK, RESET, BOLD, messages.get("msg_user_interrupt"), RESET);
        let _ = tokio::fs::remove_file(&download_path).await;
        std::process::exit(1);
    }

    print!("\r\x1b[K{}{}{}", GREEN_WD, messages.get("msg_download_completed"), RESET);
    println!();
    Ok(download_path)
}

async fn get_github_release() -> Result<(String, Value)> {
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", REPO_OWNER, REPO_NAME);
    let client = reqwest::Client::builder()
        .user_agent("Deeprotection-Updater/1.0")
        .build()?;

    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await?;
        return Err(format!("GitHub API failed: {} - {}", status, body.lines().next().unwrap_or("")).into());
    }

    let latest_release: Value = response.json().await?;
    let tag_name = latest_release["tag_name"].as_str().ok_or("No tag_name found")?;
    let clean_version = tag_name.strip_prefix('v').unwrap_or(tag_name);
    Ok((clean_version.to_string(), latest_release))
}

fn get_local_version() -> String {
    if let Ok(output) = Command::new("dp").arg("--version").output() {
        let version_output = String::from_utf8_lossy(&output.stdout);
        if let Some(captures) = regex::Regex::new(r"(\d+\.\d+\.\d+)").unwrap().captures(&version_output) {
            return captures[1].to_string();
        }
    }

    if let Ok(output) = Command::new("dpkg").args(&["-l"]).output() {
        let dpkg_output = String::from_utf8_lossy(&output.stdout);
        for line in dpkg_output.lines() {
            if line.contains("deeprotection") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    return parts[2].split('-').next().unwrap_or("0").to_string();
                }
            }
        }
    }

    "0".to_string()
}

fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    let v1_parts: Vec<u32> = v1.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    let v2_parts: Vec<u32> = v2.split('.').map(|s| s.parse().unwrap_or(0)).collect();

    for i in 0..std::cmp::max(v1_parts.len(), v2_parts.len()) {
        let v1_part = v1_parts.get(i).unwrap_or(&0);
        let v2_part = v2_parts.get(i).unwrap_or(&0);
        match v1_part.cmp(v2_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    std::cmp::Ordering::Equal
}

async fn start_update(messages: &Messages) -> Result<()> {
    let (latest_version, latest_release) = get_github_release().await?;
    let local_version = get_local_version();

    println!("{}: {}", messages.get("msg_local_version"),
        if local_version == "0" { "apt install dp".to_string() } else { local_version.clone() });
    println!("{}: {}", messages.get("msg_new_version"), latest_version);

    let needs_update = local_version == "0" || compare_versions(&local_version, &latest_version) == std::cmp::Ordering::Less;

    if needs_update {
        let assets = latest_release["assets"].as_array().ok_or("No assets found")?;
        let deb_asset = assets.iter()
            .find(|asset| asset["name"].as_str().unwrap_or("").ends_with(".deb"))
            .and_then(|asset| asset["browser_download_url"].as_str())
            .ok_or("No .deb file found")?;

        let download_path = start_download(deb_asset, messages).await?;

        print!("{}? ({}y{})/(<{}>n{}>) ",
            messages.get("ask_install_now"), GREEN_WD, RESET, RED_WD, RESET);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase().starts_with('y') {
            let has_sudo = Command::new("sudo")
                .args(&["-v"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap_or_else(|_| std::process::exit(1))
                .success();

            let status = if has_sudo {
                Command::new("sudo").args(&["dpkg", "-i", &download_path]).status()?
            } else {
                Command::new("dpkg").args(&["-i", &download_path]).status()?
            };

            if !status.success() {
                println!("{}", messages.get("err_install_fail"));
                std::process::exit(1);
            }

            let installed_version = get_local_version();
            if installed_version == latest_version {
                println!("{}{}{}", GREEN_WD, messages.get("msg_install_success"), RESET);
            } else {
                println!("{}: {} vs {}", messages.get("err_install_fail"), installed_version, latest_version);
                std::process::exit(1);
            }
        } else {
            println!("{}: {}", messages.get("msg_file_path"), download_path);
            std::process::exit(1);
        }
    } else {
        println!("{}", messages.get("msg_already_latest"));
    }

    Ok(())
}

fn get_update_value() -> Result<String> {
    let content = fs::read_to_string(CONFIG_FILE)?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("update") && line.contains('=') {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let mut value = parts[1].trim();
                if let Some(comment_pos) = value.find('#') {
                    value = &value[..comment_pos];
                }
                let value = value.trim().replace(' ', "").replace('\t', "");
                if !value.is_empty() {
                    return Ok(value);
                }
            }
        }
    }
    Err("No update configuration found".into())
}

fn check_jq_dependency() -> Result<()> {
    let output = Command::new("which")
        .arg("jq")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !output.success() {
        println!("{}sudo apt install jq{}", RED_WD, RESET);
        std::process::exit(1);
    }
    Ok(())
}

async fn check_update(ctx: &AppContext) -> Result<()> {
    check_jq_dependency()?;
    let update_val = get_update_value()?;

    match update_val.to_lowercase().as_str() {
        "disable" => {
            println!("{}", ctx.messages.get("msg_update_disabled"));
            call_dp()?;
            std::process::exit(0);
        }
        "enable" => {
            start_update(&ctx.messages).await?;
            call_dp()?;
            std::process::exit(0);
        }
        _ => {
            println!("{}: {}", ctx.messages.get("err_unknown_status"), update_val);
            std::process::exit(2);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = AppContext::new().await?;
    check_update(&ctx).await?;
    Ok(())
}
