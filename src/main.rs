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

const USAGE: &str = "用法: network-planning [--root <DIR>] <命令> [选项]

命令:
  sync-cdn   拉取 Gins-Rules CDN 快照到 gins-rules/cdn/shadowrocket
  build      抓取、去重并导出四大分类及 lowrate_override 规则文本到 rules/
  compile    调用 mihomo 与 sing-box 编译 .mrs / .srs 二进制规则
  verify     反编译并逆校验 .mrs / .srs 二进制规则
  all        依次执行 build 与 compile

选项:
  --root <DIR>        项目根目录 (默认: $NETWORK_PLANNING_ROOT、当前目录或仓库目录)
  --baseline <FILE>   build/all 对比基线 manifest.json (默认读取 rules/manifest.json)
  --max-drop <RATIO>  build/all 允许的最大条数降幅比例 (默认: 0.30，即 30%)
  --allow-drop        build/all 允许超过降幅阈值，仅告警不失败";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut command: Option<String> = None;
    let mut build_opts = builder::BuildOptions::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => match args.next() {
                Some(dir) => root = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("--root 需要目录参数\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--baseline" => match args.next() {
                Some(file) => build_opts.baseline = Some(PathBuf::from(file)),
                None => {
                    eprintln!("--baseline 需要文件路径参数\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--max-drop" => match args.next() {
                Some(raw) => match raw.parse::<f64>() {
                    Ok(ratio) if (0.0..=1.0).contains(&ratio) => build_opts.max_drop = ratio,
                    _ => {
                        eprintln!("--max-drop 必须为 0.0 到 1.0 之间的小数: {raw}\n\n{USAGE}");
                        return ExitCode::from(2);
                    }
                },
                None => {
                    eprintln!("--max-drop 需要比例参数\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--allow-drop" => build_opts.allow_drop = true,
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
        Some("build") => builder::run(&root, &build_opts),
        Some("compile") => compile::run(&root),
        Some("verify") => verify::run(&root),
        Some("all") => match builder::run(&root, &build_opts) {
            Ok(code) if code == ExitCode::SUCCESS => compile::run(&root),
            other => other,
        },
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
