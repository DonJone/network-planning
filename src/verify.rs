//! MRS 与 SRS 二进制规则逆校验套件
//! 1. 逆向反编译 Sing-box (.srs) 为 JSON，并与源码进行域名与合并 CIDR 精确对照
//! 2. 按客户端真实优先级顺序逆向校验 Sing-box 二进制规则分流结果
//! 3. 校验 Mihomo (.mrs) 二进制树结构与内核加载完整性
//! 4. 启动 Mihomo 离线内核 (REJECT 策略) 执行全量核心业务路由逆校验，失败返回非零退出码

use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant};

use ipnet::{Ipv4Net, Ipv6Net};
use serde_json::Value;

use crate::Result;
use crate::compile::{all_ruleset_keys, require_binary};
use crate::config::{LOWRATE_OVERRIDE_KEY, SOURCES};
use crate::fetch::{append_env_file, gh_error, parallel_map};

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
    ("console.aws.amazon.com", "ai", "AWS 控制台 (走 AI)"),
    ("signin.aws", "ai", "AWS 登录认证 (走 AI)"),
    ("ec2.us-west-2.amazonaws.com", "ai", "AWS EC2 区域端点 (走 AI)"),
    ("files.oaiusercontent.com", "ai", "OpenAI 文件服务 (不被 lowrate 误分流)"),
    ("api.tiktokv.com", "ai", "TikTok API (不被 lowrate 误分流)"),
    ("youtube.com", "lowrate", "YouTube 视频流媒体 (走低倍)"),
    ("googlevideo.com", "lowrate", "YouTube 视频 CDN (走低倍)"),
    ("ggpht.com", "lowrate", "YouTube 缩略图/头像 (走低倍)"),
    ("youtubei.googleapis.com", "lowrate", "YouTube API (经 ai 剔除/优先覆盖走低倍)"),
    ("photos.google.com", "lowrate", "Google 相册 (经 ai 剔除/优先覆盖走低倍)"),
    ("drive.google.com", "lowrate", "Google 网盘 (经 ai 剔除/优先覆盖走低倍)"),
    ("docs.google.com", "lowrate", "Google 文档 (经 ai 剔除/优先覆盖走低倍)"),
    ("d1d0w7op819ely.cloudfront.net", "lowrate", "第三方 CloudFront CDN (走低倍)"),
    ("github-cloud.s3.amazonaws.com", "lowrate", "GitHub Release S3 下载 (走低倍)"),
    ("s3.us-east-1.amazonaws.com", "lowrate", "AWS S3 对象存储 (走低倍)"),
    ("discord-attachments-uploads-prd.storage.googleapis.com", "lowrate", "Discord GCS 附件 (走低倍)"),
    ("calicolabs.com", "lowrate", "不被 colab 关键字误杀 (走低倍)"),
    ("netflix.com", "lowrate", "Netflix 流媒体 (走低倍)"),
    ("github.com", "lowrate", "GitHub 源码与下载 (走低倍)"),
];

fn categories() -> impl Iterator<Item = &'static str> {
    SOURCES.iter().map(|category| category.key)
}

fn load_rules(path: &Path) -> Result<Value> {
    let data: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(data.get("rules").cloned().unwrap_or(Value::Null))
}

fn json_list_len(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(arr)) => arr.len(),
        Some(Value::String(_)) => 1,
        _ => 0,
    }
}

/// 统计规则 (含嵌套逻辑规则) 中某字段的条目数，按 invert 标志区分正向/剔除规则
/// 兼容 sing-box Listable<string> 将单元素数组反编译为单个字符串的行为
fn field_len(rules: &Value, key: &str, want_invert: bool) -> usize {
    fn walk(rules: &Value, key: &str, want_invert: bool, in_invert: bool) -> usize {
        rules.as_array().map_or(0, |items| {
            items
                .iter()
                .map(|rule| {
                    let inverted = in_invert ^ (rule.get("invert").and_then(Value::as_bool) == Some(true));
                    let own = if inverted == want_invert { json_list_len(rule.get(key)) } else { 0 };
                    own + rule.get("rules").map_or(0, |nested| walk(nested, key, want_invert, inverted))
                })
                .sum()
        })
    }
    walk(rules, key, want_invert, false)
}

/// 解析 CIDR 或裸 IP (sing-box 反编译会将 /32 和 /128 省略为单 IP)
fn push_ip_or_cidr(text: &str, v4: &mut Vec<Ipv4Net>, v6: &mut Vec<Ipv6Net>) -> Result<()> {
    if text.contains('/') {
        if text.contains(':') {
            v6.push(text.parse::<Ipv6Net>().map_err(|e| format!("无效 IPv6 CIDR {text}: {e}"))?.trunc());
        } else {
            v4.push(text.parse::<Ipv4Net>().map_err(|e| format!("无效 IPv4 CIDR {text}: {e}"))?.trunc());
        }
    } else if text.contains(':') {
        v6.push(Ipv6Net::from(text.parse::<Ipv6Addr>().map_err(|e| format!("无效 IPv6 {text}: {e}"))?));
    } else {
        v4.push(Ipv4Net::from(text.parse::<Ipv4Addr>().map_err(|e| format!("无效 IPv4 {text}: {e}"))?));
    }
    Ok(())
}

/// 收集正向规则中的全部 ip_cidr 并按 IPv4/IPv6 合并规范化，与 sing-box 编译时的 CIDR 合并等价
fn collect_aggregated_cidrs(rules: &Value) -> Result<(Vec<Ipv4Net>, Vec<Ipv6Net>)> {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();

    fn walk(rules: &Value, in_invert: bool, v4: &mut Vec<Ipv4Net>, v6: &mut Vec<Ipv6Net>) -> Result<()> {
        let Some(items) = rules.as_array() else { return Ok(()) };
        for rule in items {
            let inverted = in_invert ^ (rule.get("invert").and_then(Value::as_bool) == Some(true));
            if !inverted {
                match rule.get("ip_cidr") {
                    Some(Value::Array(cidrs)) => {
                        for cidr in cidrs.iter().filter_map(Value::as_str) {
                            push_ip_or_cidr(cidr, v4, v6)?;
                        }
                    }
                    Some(Value::String(cidr)) => push_ip_or_cidr(cidr, v4, v6)?,
                    _ => {}
                }
            }
            if let Some(nested) = rule.get("rules") {
                walk(nested, inverted, v4, v6)?;
            }
        }
        Ok(())
    }

    walk(rules, false, &mut v4, &mut v6)?;
    Ok((Ipv4Net::aggregate(&v4), Ipv6Net::aggregate(&v6)))
}

fn verify_singbox(root: &Path, singbox: &Path, errors: &mut Vec<String>) -> Result<()> {
    println!("[1/3] 开始逆向解包并验证 Sing-box 二进制规则 (.srs)...");
    let rules_dir = root.join("rules").join("singbox");

    for category in all_ruleset_keys() {
        let srs = rules_dir.join(format!("{category}.srs"));
        let source = rules_dir.join(format!("{category}.json"));
        let decompiled = env::temp_dir().join(format!("np_{category}_decompiled.json"));

        let output =
            Command::new(singbox).args(["rule-set", "decompile"]).arg(&srs).arg("-o").arg(&decompiled).output()?;
        if !output.status.success() {
            let msg = format!("Sing-box 反编译失败: {category}.srs -> {}", String::from_utf8_lossy(&output.stderr).trim());
            println!("  [FAIL] {msg}");
            errors.push(msg);
            continue;
        }

        let source_rule = load_rules(&source)?;
        let decompiled_rule = load_rules(&decompiled)?;
        let _ = fs::remove_file(&decompiled);

        let src_suffix = field_len(&source_rule, "domain_suffix", false);
        let dec_suffix = field_len(&decompiled_rule, "domain_suffix", false);
        let src_domain = field_len(&source_rule, "domain", false);
        let dec_domain = field_len(&decompiled_rule, "domain", false);
        let src_kw = field_len(&source_rule, "domain_keyword", false);
        let dec_kw = field_len(&decompiled_rule, "domain_keyword", false);
        let src_ex_suffix = field_len(&source_rule, "domain_suffix", true);
        let dec_ex_suffix = field_len(&decompiled_rule, "domain_suffix", true);
        let src_ex_domain = field_len(&source_rule, "domain", true);
        let dec_ex_domain = field_len(&decompiled_rule, "domain", true);

        let (src_v4, src_v6) = collect_aggregated_cidrs(&source_rule)?;
        let (dec_v4, dec_v6) = collect_aggregated_cidrs(&decompiled_rule)?;
        let ip_count = dec_v4.len() + dec_v6.len();
        let ip_exact = src_v4 == dec_v4 && src_v6 == dec_v6;

        let domains_exact = src_suffix == dec_suffix
            && src_domain == dec_domain
            && src_kw == dec_kw
            && src_ex_suffix == dec_ex_suffix
            && src_ex_domain == dec_ex_domain;

        if domains_exact && ip_exact {
            println!(
                "  [PASS] {category:<16}.srs 逆向解包完全一致: 后缀 {dec_suffix}, 完整 {dec_domain}, 关键字 {dec_kw}, 剔除 {}, IP(合并后) {ip_count}",
                dec_ex_suffix + dec_ex_domain
            );
        } else {
            let msg = format!(
                "{category}.srs 反编译不一致: 后缀({src_suffix} vs {dec_suffix}), 完整({src_domain} vs {dec_domain}), 关键字({src_kw} vs {dec_kw}), 剔除({} vs {}), IP集合一致={ip_exact}",
                src_ex_suffix + src_ex_domain,
                dec_ex_suffix + dec_ex_domain
            );
            println!("  [FAIL] {msg}");
            errors.push(msg);
        }
    }

    println!("\n[2/3] 执行 Sing-box 二进制规则优先级分流校验 (direct -> apac -> ai -> lowrate)...");
    let ordered: Vec<&'static str> = categories().collect();
    let results: Vec<Result<Option<&'static str>>> = parallel_map(TEST_CASES, 8, |(domain, _, _)| {
        for &cat in &ordered {
            let srs = rules_dir.join(format!("{cat}.srs"));
            let output = Command::new(singbox)
                .args(["rule-set", "match"])
                .arg(&srs)
                .arg(domain)
                .args(["--format", "binary"])
                .output()?;
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            if text.contains("match rules") {
                return Ok(Some(cat));
            }
        }
        Ok(None)
    });

    for ((domain, expected, description), actual) in TEST_CASES.iter().zip(results) {
        let actual = actual?;
        if actual == Some(*expected) {
            println!("  [MATCH OK]   {domain:<54} -> {:<7} ({description})", expected.to_uppercase());
        } else {
            let got = actual.unwrap_or("NONE");
            let msg = format!("Sing-box 分流失败: {domain} 实际命中 {got}, 期望 {expected} ({description})");
            println!("  [MATCH FAIL] {msg}");
            errors.push(msg);
        }
    }
    Ok(())
}

fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn mihomo_config(root: &Path, port: u16) -> String {
    let rules_dir = root.join("rules").join("mihomo");
    let has_override = rules_dir.join(format!("{LOWRATE_OVERRIDE_KEY}.mrs")).is_file();
    let mut config = format!("mixed-port: {port}\nmode: rule\nlog-level: info\nrule-providers:\n");

    for category in all_ruleset_keys() {
        for (suffix, behavior) in [("", "domain"), ("_ip", "ipcidr")] {
            let path = rules_dir.join(format!("{category}{suffix}.mrs"));
            if !path.is_file() {
                continue;
            }
            let _ = write!(
                config,
                "  rule_{category}{suffix}:\n    type: file\n    behavior: {behavior}\n    format: mrs\n    path: {}\n",
                path.display()
            );
        }
    }

    config.push_str("rules:\n");
    for category in categories() {
        if category == "ai" && has_override {
            let _ = writeln!(config, "  - RULE-SET,rule_{LOWRATE_OVERRIDE_KEY},REJECT");
        }
        let _ = writeln!(config, "  - RULE-SET,rule_{category},REJECT");
    }
    for category in categories() {
        if rules_dir.join(format!("{category}_ip.mrs")).is_file() {
            let _ = writeln!(config, "  - RULE-SET,rule_{category}_ip,REJECT,no-resolve");
        }
    }
    config.push_str("  - MATCH,REJECT\n");
    config
}

fn wait_for_port(port: u16, max_wait: Duration) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + max_wait;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// 经 Mihomo HTTP 代理发起一次离线探针请求 (命中规则直接 REJECT，无外网依赖)
fn probe(port: u16, domain: &str) {
    let timeout = Duration::from_millis(800);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else { return };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request =
        format!("GET http://{domain}/ HTTP/1.1\r\nHost: {domain}\r\nUser-Agent: curl/7.88\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_ok() {
        let mut buffer = [0u8; 256];
        let _ = stream.read(&mut buffer);
    }
}

fn extract_matched_ruleset(log_content: &str, domain: &str) -> Option<String> {
    let target = format!("--> {domain}:");
    for line in log_content.lines() {
        if !line.contains(&target) {
            continue;
        }
        for prefix in ["match RuleSet(", "match RuleSet/"] {
            if let Some(idx) = line.find(prefix) {
                let rest = &line[idx + prefix.len()..];
                if let Some(end) = rest.find(')') {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
}

fn verify_mihomo(root: &Path, mihomo: &Path, errors: &mut Vec<String>) -> Result<()> {
    println!("\n[3/3] 启动 Mihomo 内核执行离线分流逆校验 (含 lowrate_override)...");
    let port = pick_free_port()?;
    let config_path = env::temp_dir().join(format!("verify_mihomo_{port}.yaml"));
    let log_path = env::temp_dir().join(format!("verify_mihomo_{port}.log"));
    fs::write(&config_path, mihomo_config(root, port))?;

    // 1. 语法与载入测试
    let test = Command::new(mihomo).arg("-t").arg("-f").arg(&config_path).output()?;
    if !test.status.success() {
        let msg = format!(
            "Mihomo 配置测试失败: {}{}",
            String::from_utf8_lossy(&test.stdout),
            String::from_utf8_lossy(&test.stderr)
        );
        println!("  [FAIL] {msg}");
        errors.push(msg);
        let _ = fs::remove_file(&config_path);
        return Ok(());
    }
    println!("  [PASS] Mihomo 成功载入全量 .mrs 二进制规则集 (LOUDS 树构建成功)");

    // 2. 启动临时实例测试路由匹配 (使用 REJECT 策略，完全不依赖外网)
    let log = File::create(&log_path)?;
    let mut child = Command::new(mihomo).arg("-f").arg(&config_path).stdout(log.try_clone()?).stderr(log).spawn()?;
    if !wait_for_port(port, Duration::from_secs(5)) {
        let _ = child.kill();
        let _ = child.wait();
        let msg = format!("Mihomo 测试实例在端口 {port} 启动超时");
        println!("  [FAIL] {msg}");
        errors.push(msg);
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_file(&log_path);
        return Ok(());
    }

    for (domain, _, _) in TEST_CASES {
        probe(port, domain);
    }
    thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
    let _ = child.wait();

    // 3. 分析内核日志，精确核对命中的规则集标签
    let log_content = String::from_utf8_lossy(&fs::read(&log_path)?).into_owned();
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&log_path);

    let override_tag = format!("rule_{LOWRATE_OVERRIDE_KEY}");
    for (domain, expected, description) in TEST_CASES {
        let expected_tag = format!("rule_{expected}");
        let actual = extract_matched_ruleset(&log_content, domain);
        let ok = match actual.as_deref() {
            Some(tag) if tag == expected_tag => true,
            Some(tag) if *expected == "lowrate" && tag == override_tag => true,
            _ => false,
        };
        if ok {
            let tag = actual.as_deref().unwrap_or(&expected_tag);
            println!("  [KERNEL OK]   {domain:<54} -> {tag:<22} ({description})");
        } else {
            let got = actual.as_deref().unwrap_or("未命中");
            let msg = format!("Mihomo 分流失败: {domain} 实际命中 {got}, 期望 {expected_tag} ({description})");
            println!("  [KERNEL FAIL] {msg}");
            errors.push(msg);
        }
    }
    Ok(())
}

fn write_verify_summary(errors: &[String]) -> Result<()> {
    let mut md = format!(
        "## 二进制规则逆校验 (verify)\n\n- 测试用例: {} 条 (Sing-box + Mihomo 双端验证)\n- 校验结果: {}\n",
        TEST_CASES.len(),
        if errors.is_empty() { "全部通过".to_string() } else { format!("失败 {} 项", errors.len()) }
    );
    if !errors.is_empty() {
        md.push_str("\n### 失败明细\n\n");
        for err in errors {
            let _ = writeln!(md, "- {err}");
        }
    }
    md.push('\n');
    append_env_file("GITHUB_STEP_SUMMARY", &md)
}

pub fn run(root: &Path) -> Result<ExitCode> {
    let rule = "=".repeat(65);
    println!("{rule}\n      MRS & SRS 跨平台二进制规则反向解包与逆校验套件\n{rule}");

    let mut errors = Vec::new();
    verify_singbox(root, &require_binary(root, "sing-box")?, &mut errors)?;
    verify_mihomo(root, &require_binary(root, "mihomo")?, &mut errors)?;
    write_verify_summary(&errors)?;

    if !errors.is_empty() {
        for err in &errors {
            gh_error("规则逆校验失败", err);
        }
        eprintln!("\n{rule}\n   二进制规则逆校验失败：共 {} 项错误！\n{rule}", errors.len());
        return Ok(ExitCode::FAILURE);
    }

    println!("\n{rule}\n              全量二进制规则逆校验全部通过！\n{rule}");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mihomo_ruleset_from_logs() {
        let log = r#"
time="2026-11-26T18:32:03+08:00" level=info msg="[TCP] 127.0.0.1:59604 --> photos.google.com:80 match RuleSet(rule_lowrate_override) using REJECT"
time="2026-11-26T18:32:03+08:00" level=info msg="[TCP] 127.0.0.1:59605 --> chat.openai.com:80 (match RuleSet/rule_ai) error"
"#;
        assert_eq!(extract_matched_ruleset(log, "photos.google.com").as_deref(), Some("rule_lowrate_override"));
        assert_eq!(extract_matched_ruleset(log, "chat.openai.com").as_deref(), Some("rule_ai"));
        assert_eq!(extract_matched_ruleset(log, "missing.example.com"), None);
    }

    #[test]
    fn aggregates_cidrs_identically() {
        let a = serde_json::json!([{"ip_cidr": ["1.1.1.0/25", "1.1.1.128/25", "8.8.8.8/32", "2001:db8::/33", "2001:db8:8000::/33", "2001:db9::1/128"]}]);
        let b = serde_json::json!([{"ip_cidr": ["1.1.1.0/24", "8.8.8.8", "2001:db8::/32", "2001:db9::1"]}]);
        assert_eq!(collect_aggregated_cidrs(&a).unwrap(), collect_aggregated_cidrs(&b).unwrap());
    }
}
