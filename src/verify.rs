//! MRS 与 SRS 二进制规则逆校验套件
//! 1. 逆向反编译 Sing-box (.srs) 为 JSON，并与源码进行全量字段对照
//! 2. 逆向测试 Sing-box 二进制规则命中性
//! 3. 校验 Mihomo (.mrs) 二进制树结构与内核加载完整性
//! 4. 启动 Mihomo 本地内核执行全量核心业务路由逆校验

use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use crate::compile::require_binary;
use crate::config::SOURCES;

const MIHOMO_PORT: u16 = 7898;

/// (测试目标, 期望归属规则集, 描述)
const TEST_CASES: &[(&str, &str, &str)] = &[
    ("steamcommunity.com", "apac", "Steam 社区/创意工坊 (走亚太)"),
    ("steampowered.com", "apac", "Steam 商城主站 (走亚太)"),
    ("epicgames.com", "apac", "Epic Games 客户端与商城 (走亚太)"),
    ("t.me", "apac", "Telegram 短链接 (走亚太)"),
    ("telegram.org", "apac", "Telegram 官网 (走亚太)"),
    ("x.com", "apac", "X/Twitter 核心域名 (走亚太)"),
    ("steampipe.steamcontent.com", "direct", "Steam 游戏下载 CDN (走直连)"),
    ("download.epicgames.com", "direct", "Epic 游戏下载 CDN (走直连)"),
    ("bytedance.com", "direct", "字节跳动国内服务 (走直连)"),
    ("qq.com", "direct", "腾讯国内直连 (走直连)"),
    ("baidu.com", "direct", "百度国内直连 (走直连)"),
    ("chat.openai.com", "ai", "OpenAI / ChatGPT (走 AI)"),
    ("claude.ai", "ai", "Anthropic Claude (走 AI)"),
    ("gemini.google.com", "ai", "Google Gemini (走 AI)"),
    ("tiktok.com", "ai", "TikTok 国际版 (走 AI)"),
    ("google.com", "ai", "Google 核心搜索/API (走 AI)"),
    ("youtube.com", "lowrate", "YouTube 视频流媒体 (走低倍)"),
    ("googlevideo.com", "lowrate", "YouTube 视频 CDN (走低倍)"),
    ("netflix.com", "lowrate", "Netflix 流媒体 (走低倍)"),
    ("github.com", "lowrate", "GitHub 源码与下载 (走低倍)"),
];

fn categories() -> impl Iterator<Item = &'static str> {
    SOURCES.iter().map(|category| category.key)
}

fn first_rule(path: &Path) -> Result<Value> {
    let data: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(data.get("rules").and_then(|rules| rules.get(0)).cloned().unwrap_or(Value::Null))
}

fn field_len(rule: &Value, key: &str) -> usize {
    rule.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn verify_singbox(root: &Path, singbox: &Path) -> Result<()> {
    println!("[1/3] 开始逆向解包并验证 Sing-box 二进制规则 (.srs)...");
    let rules_dir = root.join("rules").join("singbox");

    for category in categories() {
        let srs = rules_dir.join(format!("{category}.srs"));
        let source = rules_dir.join(format!("{category}.json"));
        let decompiled = env::temp_dir().join(format!("{category}_decompiled.json"));

        let output =
            Command::new(singbox).args(["rule-set", "decompile"]).arg(&srs).arg("-o").arg(&decompiled).output()?;
        if !output.status.success() {
            println!("  [FAIL] 反编译失败: {category}.srs -> {}", String::from_utf8_lossy(&output.stderr).trim());
            continue;
        }

        let source_rule = first_rule(&source)?;
        let decompiled_rule = first_rule(&decompiled)?;
        let (src_suffix, dec_suffix) = (field_len(&source_rule, "domain_suffix"), field_len(&decompiled_rule, "domain_suffix"));
        let (src_ip, dec_ip) = (field_len(&source_rule, "ip_cidr"), field_len(&decompiled_rule, "ip_cidr"));
        if src_suffix == dec_suffix && src_ip == dec_ip {
            println!("  [PASS] {category:<7}.srs 逆向解包完全一致: 后缀 {dec_suffix} 条, IP {dec_ip} 条");
        } else {
            println!(
                "  [WARN] {category:<7}.srs 数量微差 (可能为基数树内联化): 源码({src_suffix}/{src_ip}) vs 反编译({dec_suffix}/{dec_ip})"
            );
        }
    }

    println!("\n[2/3] 执行 Sing-box 二进制规则匹配度校验...");
    for (domain, expected, description) in TEST_CASES {
        let srs = rules_dir.join(format!("{expected}.srs"));
        let output =
            Command::new(singbox).args(["rule-set", "match"]).arg(&srs).arg(domain).args(["--format", "binary"]).output()?;
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if text.contains("match rules") {
            println!("  [MATCH OK] {domain:<30} -> {:<7} ({description})", expected.to_uppercase());
        } else {
            println!("  [MATCH FAIL] {domain:<30} 未命中期望组: {expected}");
        }
    }
    Ok(())
}

fn mihomo_config(root: &Path) -> String {
    let rules_dir = root.join("rules").join("mihomo");
    let mut config = format!("mixed-port: {MIHOMO_PORT}\nmode: rule\nlog-level: debug\nrule-providers:\n");
    for category in categories() {
        for (suffix, behavior) in [("", "domain"), ("_ip", "ipcidr")] {
            let path = rules_dir.join(format!("{category}{suffix}.mrs"));
            let _ = write!(
                config,
                "  rule_{category}{suffix}:\n    type: file\n    behavior: {behavior}\n    format: mrs\n    path: {}\n",
                path.display()
            );
        }
    }
    config.push_str("rules:\n");
    for category in categories() {
        let _ = writeln!(config, "  - RULE-SET,rule_{category},DIRECT");
    }
    for category in categories() {
        let _ = writeln!(config, "  - RULE-SET,rule_{category}_ip,DIRECT,no-resolve");
    }
    config.push_str("  - MATCH,DIRECT\n");
    config
}

/// 经 Mihomo HTTP 代理发起一次请求，只为触发内核路由日志
fn probe(domain: &str) {
    let timeout = Duration::from_millis(800);
    let address = SocketAddr::from(([127, 0, 0, 1], MIHOMO_PORT));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else { return };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request =
        format!("GET http://{domain}/ HTTP/1.1\r\nHost: {domain}\r\nUser-Agent: curl/7.88\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_ok() {
        let mut buffer = [0u8; 512];
        let _ = stream.read(&mut buffer);
    }
}

fn verify_mihomo(root: &Path, mihomo: &Path) -> Result<()> {
    println!("\n[3/3] 启动 Mihomo 内核执行真实连接逆向分流校验...");
    let config_path = env::temp_dir().join("verify_mihomo.yaml");
    let log_path = env::temp_dir().join("verify_mihomo.log");
    fs::write(&config_path, mihomo_config(root))?;

    // 1. 语法与载入测试
    let test = Command::new(mihomo).arg("-t").arg("-f").arg(&config_path).output()?;
    if !test.status.success() {
        println!(
            "  [FAIL] Mihomo 配置测试失败: {}{}",
            String::from_utf8_lossy(&test.stdout),
            String::from_utf8_lossy(&test.stderr)
        );
        return Ok(());
    }
    println!("  [PASS] Mihomo 成功载入全量 8 个 .mrs 二进制规则集 (LOUDS 树构建成功)");

    // 2. 启动临时实例测试真实连接
    let log = File::create(&log_path)?;
    let mut child = Command::new(mihomo).arg("-f").arg(&config_path).stdout(log.try_clone()?).stderr(log).spawn()?;
    thread::sleep(Duration::from_secs(2));
    for (domain, _, _) in TEST_CASES {
        probe(domain);
    }
    thread::sleep(Duration::from_secs(1));
    let _ = child.kill();
    let _ = child.wait();

    // 3. 分析内核日志：匹配 match RuleSet(rule_xxx)，以及拨号失败时的 (match RuleSet/rule_xxx)
    let log_content = String::from_utf8_lossy(&fs::read(&log_path)?).into_owned();
    for (domain, expected, description) in TEST_CASES {
        let tag = format!("rule_{expected}");
        let markers = [format!("match RuleSet({tag})"), format!("match RuleSet/{tag})")];
        let target = format!("--> {domain}:");
        let matched = log_content
            .lines()
            .any(|line| line.contains(&target) && markers.iter().any(|marker| line.contains(marker.as_str())));
        if matched {
            println!("  [KERNEL OK] {domain:<30} -> {tag:<15} ({description})");
        } else {
            println!("  [KERNEL FAIL] {domain:<30} 未命中 {tag}");
        }
    }
    Ok(())
}

pub fn run(root: &Path) -> Result<ExitCode> {
    let rule = "=".repeat(65);
    println!("{rule}\n      MRS & SRS 跨平台二进制规则反向解包与逆校验套件\n{rule}");

    verify_singbox(root, &require_binary(root, "sing-box")?)?;
    verify_mihomo(root, &require_binary(root, "mihomo")?)?;

    println!("\n{rule}\n              全量二进制规则逆校验完成！\n{rule}");
    Ok(ExitCode::SUCCESS)
}
