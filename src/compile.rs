//! 调用 mihomo 与 sing-box 将规则文本编译为 .mrs / .srs 二进制规则集

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::Result;
use crate::config::{LOWRATE_OVERRIDE_KEY, SOURCES};

pub fn all_ruleset_keys() -> Vec<&'static str> {
    SOURCES.iter().map(|category| category.key).chain(std::iter::once(LOWRATE_OVERRIDE_KEY)).collect()
}

/// 优先使用仓库 bin/ 下的二进制，其次在 PATH 中查找
pub fn require_binary(root: &Path, name: &str) -> Result<PathBuf> {
    let local = root.join("bin").join(name);
    if local.is_file() {
        return Ok(local);
    }
    env::var_os("PATH")
        .and_then(|paths| env::split_paths(&paths).map(|dir| dir.join(name)).find(|path| path.is_file()))
        .ok_or_else(|| format!("未找到 {name} 二进制！请放置于 bin/{name} 或加入 PATH").into())
}

fn check(command: &mut Command) -> Result<()> {
    let status = command.status().map_err(|err| format!("无法执行 {:?}: {err}", command.get_program()))?;
    if !status.success() {
        return Err(format!("命令执行失败 ({status}): {command:?}").into());
    }
    Ok(())
}

pub fn run(root: &Path) -> Result<ExitCode> {
    let mihomo = require_binary(root, "mihomo")?;
    let singbox = require_binary(root, "sing-box")?;
    let mihomo_dir = root.join("rules").join("mihomo");
    let singbox_dir = root.join("rules").join("singbox");

    let keys = all_ruleset_keys();

    println!("[*] 开始编译 Mihomo 二进制规则集 (.mrs)...");
    for category in &keys {
        let domain_source = mihomo_dir.join(format!("{category}.yaml"));
        if domain_source.is_file() {
            println!("  -> 编译 Mihomo: {category}.mrs (domain)");
            check(
                Command::new(&mihomo)
                    .args(["convert-ruleset", "domain", "yaml"])
                    .arg(&domain_source)
                    .arg(mihomo_dir.join(format!("{category}.mrs"))),
            )?;
        }

        let ip_source = mihomo_dir.join(format!("{category}_ip.yaml"));
        if ip_source.is_file() {
            println!("  -> 编译 Mihomo: {category}_ip.mrs (ipcidr)");
            check(
                Command::new(&mihomo)
                    .args(["convert-ruleset", "ipcidr", "yaml"])
                    .arg(&ip_source)
                    .arg(mihomo_dir.join(format!("{category}_ip.mrs"))),
            )?;
        }
    }

    println!("[*] 开始编译 Sing-box 二进制规则集 (.srs)...");
    for category in &keys {
        let source = singbox_dir.join(format!("{category}.json"));
        if source.is_file() {
            println!("  -> 编译 Sing-box: {category}.srs");
            check(
                Command::new(&singbox)
                    .args(["rule-set", "compile"])
                    .arg(&source)
                    .arg("-o")
                    .arg(singbox_dir.join(format!("{category}.srs"))),
            )?;
        }
    }

    println!("[*] 全部二进制规则编译完成！");
    Ok(ExitCode::SUCCESS)
}
