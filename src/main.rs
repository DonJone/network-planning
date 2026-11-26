mod builder;
mod compile;
mod config;
mod export;
mod fetch;
mod parser;
mod sync_cdn;
mod verify;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const USAGE: &str = "用法: network-planning [--root <DIR>] <命令>

命令:
  sync-cdn   拉取 Gins-Rules CDN 快照到 gins-rules/cdn/shadowrocket
  build      抓取、去重并导出四大分类规则文本到 rules/
  compile    调用 mihomo 与 sing-box 编译 .mrs / .srs 二进制规则
  verify     反编译并逆校验 .mrs / .srs 二进制规则
  all        依次执行 build 与 compile

选项:
  --root <DIR>  项目根目录 (默认: $NETWORK_PLANNING_ROOT、当前目录或仓库目录)";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut command: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => match args.next() {
                Some(dir) => root = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("--root 需要目录参数\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if command.is_none() => command = Some(arg),
            _ => {
                eprintln!("未知参数: {arg}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let root = root.unwrap_or_else(default_root);
    let result = match command.as_deref() {
        Some("sync-cdn") => sync_cdn::run(&root),
        Some("build") => builder::run(&root),
        Some("compile") => compile::run(&root),
        Some("verify") => verify::run(&root),
        Some("all") => builder::run(&root).and_then(|_| compile::run(&root)),
        Some(other) => {
            eprintln!("未知命令: {other}\n\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("[ERROR] {err}");
            ExitCode::FAILURE
        }
    }
}

fn default_root() -> PathBuf {
    if let Some(dir) = env::var_os("NETWORK_PLANNING_ROOT") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = env::current_dir() {
        if cwd.join("custom").is_dir() && cwd.join("gins-rules").is_dir() {
            return cwd;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
