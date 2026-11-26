//! 规则集主构建流程
//! 负责拉取、解构、清洗、冲突判定、合并去重、降幅阈值校验，
//! 并生成 Mihomo、Loon、Shadowrocket、Sing-box 的四大块及 lowrate_override 规则文件。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config::{
    AI_EXCLUDED_PARENT_SUFFIXES, GINS_FORCE_PROXY, LOWRATE_CATCHALL_GINS, LOWRATE_CATCHALL_REMOTE,
    LOWRATE_OVERRIDE_KEY, OVERRIDE_DIRECT_PATTERNS, SOURCES,
};
use crate::export::{self, Bucket};
use crate::fetch::{append_env_file, fetch_text_with_fallback, gh_error, gh_warning, parallel_map};
use crate::parser::{Kind, Rule, file_lines, parse_gins_line, parse_line, splitlines};

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";
const DEFAULT_MAX_DROP: f64 = 0.30;
const MIN_DROP_BASELINE: usize = 10;

const APPLE_PROXY_SERVICES: &[&str] = &[
    "sync.itunes.apple.com",
    "upp.itunes.apple.com",
    "play.itunes.apple.com",
    "api.appstore.apple.com",
    "bag.itunes.apple.com",
    "buy.itunes.apple.com",
    "itunes.apple.com",
    "music.apple.com",
    "amp-api.music.apple.com",
];

static AWS_REGION_ROOT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]{2}(-[a-z]+)+-\d+\.amazonaws\.com$").expect("valid aws region regex"));

pub struct BuildOptions {
    pub baseline: Option<PathBuf>,
    pub max_drop: f64,
    pub allow_drop: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self { baseline: None, max_drop: DEFAULT_MAX_DROP, allow_drop: false }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryStats {
    pub suffixes: usize,
    pub domains: usize,
    pub keywords: usize,
    pub ips: usize,
    #[serde(default)]
    pub exclude_suffixes: usize,
    #[serde(default)]
    pub exclude_domains: usize,
    pub total: usize,
}

impl CategoryStats {
    fn from_bucket(bucket: &Bucket) -> Self {
        let suffixes = bucket.suffixes.len();
        let domains = bucket.domains.len();
        let keywords = bucket.keywords.len();
        let ips = bucket.ips.len();
        Self {
            suffixes,
            domains,
            keywords,
            ips,
            exclude_suffixes: bucket.exclude_suffixes.len(),
            exclude_domains: bucket.exclude_domains.len(),
            total: suffixes + domains + keywords + ips,
        }
    }

    fn domain_total(&self) -> usize {
        self.suffixes + self.domains
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildManifest {
    pub version: u8,
    pub sources: BTreeMap<String, usize>,
    pub categories: BTreeMap<String, CategoryStats>,
}

struct RemoteFetchReport {
    url: &'static str,
    used_url: String,
    fell_back: bool,
    primary_error: Option<String>,
    rules: Vec<Rule>,
    error: Option<String>,
}

fn fetch_rules(url: &'static str) -> RemoteFetchReport {
    match fetch_text_with_fallback(url, USER_AGENT, 3, Duration::from_secs(25), Duration::from_secs(1)) {
        Ok(outcome) => {
            let rules: Vec<Rule> = splitlines(&outcome.text).into_iter().filter_map(parse_line).collect();
            if outcome.fell_back {
                println!(
                    "  [WARN] 主地址失败，已回退镜像: {url} -> {} ({} 条)",
                    outcome.used_url,
                    rules.len()
                );
            }
            RemoteFetchReport {
                url,
                used_url: outcome.used_url,
                fell_back: outcome.fell_back,
                primary_error: outcome.primary_error,
                rules,
                error: None,
            }
        }
        Err(err) => {
            println!("  [ERROR] 拉取最终失败: {url} -> {err}");
            RemoteFetchReport {
                url,
                used_url: url.to_string(),
                fell_back: false,
                primary_error: None,
                rules: Vec::new(),
                error: Some(err),
            }
        }
    }
}

fn parse_file(path: &Path, parse: fn(&str) -> Option<Rule>) -> Result<Vec<Rule>> {
    let text = fs::read_to_string(path).map_err(|err| format!("读取 {} 失败: {err}", path.display()))?;
    Ok(file_lines(&text).into_iter().filter_map(parse).collect())
}

/// 读取本地自定义规则文件，不存在时返回空
fn load_local_file(root: &Path, relative: &str) -> Result<Vec<Rule>> {
    let path = root.join(relative);
    if !path.exists() {
        return Ok(Vec::new());
    }
    parse_file(&path, parse_line)
}

fn is_gins_force_proxy(domain: &str) -> bool {
    domain.contains("tiktok")
        || domain.contains("tik-tok")
        || GINS_FORCE_PROXY
            .iter()
            .any(|proxy| domain == *proxy || domain.strip_suffix(proxy).is_some_and(|rest| rest.ends_with('.')))
}

fn is_ai_excluded_parent(domain: &str) -> bool {
    AI_EXCLUDED_PARENT_SUFFIXES
        .iter()
        .any(|suffix| domain == *suffix || domain.strip_suffix(suffix).is_some_and(|rest| rest.ends_with('.')))
}

/// 判断是否为多租户云存储 / CDN / 托管子域 (例如 S3 桶、ELB、GCS、ElasticBeanstalk、GlobalAccelerator)
/// 严格排除裸 AWS 区域根域名 (如 us-west-2.amazonaws.com)，避免误将 EC2/Bedrock/QuickSight 踢出 ai
fn is_shared_cloud_tenant_subdomain(domain: &str) -> bool {
    if AWS_REGION_ROOT_RE.is_match(domain) {
        return false;
    }
    if let Some(prefix) = domain.strip_suffix(".amazonaws.com") {
        return prefix.starts_with("s3.")
            || prefix.starts_with("s3-")
            || prefix.contains(".s3.")
            || prefix.contains(".s3-")
            || prefix.ends_with(".s3")
            || prefix.contains(".elb.");
    }
    domain.ends_with(".awsglobalaccelerator.com")
        || domain.ends_with(".elasticbeanstalk.com")
        || domain.ends_with(".storage.googleapis.com")
        || domain.ends_with(".cloudfront.net")
}

/// 读取内置 Gins-Rules 源与 CDN 快照，reference 格式为 分类/规则名 (如 proxy/steam)
fn load_gins_rule(root: &Path, reference: &str) -> Result<Vec<Rule>> {
    let (category, name) = reference
        .split_once('/')
        .ok_or_else(|| format!("Gins 源格式错误 (应为 分类/规则名): {reference}"))?;
    let source_category = if category == "asn" { "ip" } else { category };
    let gins_dir = root.join("gins-rules");
    let sources = [
        gins_dir.join("source").join(source_category).join(format!("{name}.txt")),
        gins_dir.join("source").join("upstream").join(source_category).join(format!("{name}.txt")),
    ];
    let snapshot = gins_dir.join("cdn").join("shadowrocket").join(source_category).join(format!("{name}.list"));

    let existing: Vec<_> = sources.iter().filter(|path| path.exists()).collect();
    if existing.is_empty() && !snapshot.exists() {
        println!("  [WARN] Gins 源不存在: {reference}");
        return Ok(Vec::new());
    }

    let mut rules = Vec::new();
    for path in existing {
        rules.extend(parse_file(path, parse_gins_line)?);
    }
    if snapshot.exists() {
        rules.extend(parse_file(&snapshot, parse_line)?);
    }
    if category != "proxy" {
        rules.retain(|(kind, value)| !(kind.is_domain() && is_gins_force_proxy(value)));
    }
    Ok(rules)
}

/// 依次产出域名本身及其各级父域 (a.b.c -> a.b.c, b.c, c)
fn domain_suffixes(domain: &str) -> impl Iterator<Item = &str> {
    std::iter::once(domain).chain(domain.match_indices('.').map(move |(index, _)| &domain[index + 1..]))
}

/// 分离并去重域名、后缀、关键词与 IP
fn deduplicate(rules: &[Rule]) -> Bucket {
    let mut suffixes = BTreeSet::new();
    let mut domains = BTreeSet::new();
    let mut keywords = BTreeSet::new();
    let mut ips = BTreeSet::new();
    for (kind, value) in rules {
        let set = match kind {
            Kind::DomainSuffix => &mut suffixes,
            Kind::Domain => &mut domains,
            Kind::Keyword => &mut keywords,
            Kind::IpCidr => &mut ips,
        };
        set.insert(value.as_str());
    }

    // 域名树优化：如果已有父级 DOMAIN-SUFFIX，子域名 DOMAIN 无需重复保留
    domains.retain(|domain| !domain_suffixes(domain).skip(1).any(|parent| suffixes.contains(parent)));

    let owned = |set: BTreeSet<&str>| set.into_iter().map(str::to_owned).collect();
    Bucket {
        suffixes: owned(suffixes),
        domains: owned(domains),
        keywords: owned(keywords),
        ips: owned(ips),
        exclude_suffixes: Vec::new(),
        exclude_domains: Vec::new(),
    }
}

/// 计算被 ai 父域或关键字覆盖、但实际应走 lowrate 的优先分流集合：
/// 1. lowrate_custom 手写规则 (如 Google Photos / Drive / Docs / youtubei)
/// 2. lowrate 专用业务源 (YouTube / Media / GitHub / Netflix / Disney+ 等，跳过裸 AWS 区域根域名)
/// 3. lowrate 泛代理源中的多租户云存储/CDN/托管子域 (S3 / ELB / GCS / ElasticBeanstalk / GlobalAccelerator)
/// 4. 仅被 ai 关键字误杀的 lowrate 域名 (如 colab 误杀 calicolabs.com)
/// 结果同步写入 ai.exclude_* (供 Sing-box AND NOT) 并导出为 lowrate_override (供全平台优先匹配)
fn compute_lowrate_overrides(
    ai_bucket: &mut Bucket,
    lowrate_bucket: &Bucket,
    lowrate_custom_rules: &[Rule],
    lowrate_authoritative_domains: &HashSet<String>,
) -> Bucket {
    let ai_suffixes: HashSet<&str> = ai_bucket.suffixes.iter().map(String::as_str).collect();
    let covered_by_suffix = |domain: &str| domain_suffixes(domain).any(|parent| ai_suffixes.contains(parent));
    let covered_by_keyword = |domain: &str| ai_bucket.keywords.iter().any(|kw| domain.contains(kw.as_str()));

    let should_override = |domain: &str, force_custom: bool| -> bool {
        if AWS_REGION_ROOT_RE.is_match(domain) {
            return false;
        }
        let by_suffix = covered_by_suffix(domain);
        let by_keyword = covered_by_keyword(domain);
        if !by_suffix && !by_keyword {
            return false;
        }
        if force_custom {
            return true;
        }
        // 不得把与 ai 自身相等的父级后缀踢出 ai
        if ai_suffixes.contains(domain) {
            return false;
        }
        if !by_suffix && by_keyword {
            return true;
        }
        lowrate_authoritative_domains.contains(domain) || is_shared_cloud_tenant_subdomain(domain)
    };

    let mut override_rules: Vec<Rule> = Vec::new();
    for (kind, value) in lowrate_custom_rules {
        if kind.is_domain() && should_override(value, true) {
            override_rules.push((*kind, value.clone()));
        }
    }
    for suffix in &lowrate_bucket.suffixes {
        if should_override(suffix, false) {
            override_rules.push((Kind::DomainSuffix, suffix.clone()));
        }
    }
    for domain in &lowrate_bucket.domains {
        if should_override(domain, false) {
            override_rules.push((Kind::Domain, domain.clone()));
        }
    }

    let override_bucket = deduplicate(&override_rules);
    ai_bucket.exclude_suffixes = override_bucket.suffixes.clone();
    ai_bucket.exclude_domains = override_bucket.domains.clone();
    override_bucket
}

fn check_drop(label: &str, prev: usize, curr: usize, max_drop: f64) -> Option<String> {
    if prev == 0 || curr >= prev {
        return None;
    }
    let dropped = prev - curr;
    let ratio = dropped as f64 / prev as f64;
    if curr == 0 || (prev >= MIN_DROP_BASELINE && ratio > max_drop) {
        return Some(format!(
            "{label} 条数异常下降: {prev} -> {curr} (-{:.1}%, 阈值 {:.0}%)",
            ratio * 100.0,
            max_drop * 100.0
        ));
    }
    None
}

fn validate_against_baseline(
    baseline: &BuildManifest,
    current: &BuildManifest,
    max_drop: f64,
) -> Vec<String> {
    let mut violations = Vec::new();
    for (source, &curr) in &current.sources {
        if let Some(&prev) = baseline.sources.get(source) {
            if let Some(msg) = check_drop(&format!("源 {source}"), prev, curr, max_drop) {
                violations.push(msg);
            }
        }
    }
    for category in SOURCES {
        let key = category.key;
        let (Some(prev), Some(curr)) = (baseline.categories.get(key), current.categories.get(key)) else {
            continue;
        };
        if let Some(msg) = check_drop(&format!("分类 {key} 总条数"), prev.total, curr.total, max_drop) {
            violations.push(msg);
        }
        if let Some(msg) = check_drop(&format!("分类 {key} 域名条数"), prev.domain_total(), curr.domain_total(), max_drop) {
            violations.push(msg);
        }
        if let Some(msg) = check_drop(&format!("分类 {key} IP 条数"), prev.ips, curr.ips, max_drop) {
            violations.push(msg);
        }
    }
    violations
}

fn format_delta(prev: Option<usize>, curr: usize) -> String {
    match prev {
        None => "新".to_string(),
        Some(prev) if curr == prev => "0".to_string(),
        Some(prev) if curr > prev => format!("+{}", curr - prev),
        Some(prev) => {
            let diff = prev - curr;
            let pct = if prev > 0 { diff as f64 * 100.0 / prev as f64 } else { 0.0 };
            format!("-{diff} (-{pct:.1}%)")
        }
    }
}

fn load_baseline(root: &Path, options: &BuildOptions) -> Result<Option<BuildManifest>> {
    let candidate = match &options.baseline {
        Some(path) => path.clone(),
        None => root.join("rules").join("manifest.json"),
    };
    if !candidate.is_file() {
        println!("[*] 未找到基线 manifest ({})，本次仅检查源拉取完整性", candidate.display());
        return Ok(None);
    }
    let text = fs::read_to_string(&candidate)
        .map_err(|err| format!("读取基线 manifest {} 失败: {err}", candidate.display()))?;
    let manifest: BuildManifest = serde_json::from_str(&text)
        .map_err(|err| format!("解析基线 manifest {} 失败: {err}", candidate.display()))?;
    println!("[*] 已加载基线 manifest: {} (最大允许降幅 {:.0}%)", candidate.display(), options.max_drop * 100.0);
    Ok(Some(manifest))
}

fn write_build_summary(
    reports: &[RemoteFetchReport],
    manifest: &BuildManifest,
    baseline: Option<&BuildManifest>,
    fetch_errors: &[String],
    drop_violations: &[String],
    allow_drop: bool,
) -> Result<()> {
    let mut md = String::from("## 规则集构建报告\n\n");
    md.push_str("### 分类统计\n\n");
    md.push_str("| 分类 | 域名后缀 | 完整域名 | 关键字 | IP | 总计 | 较基线变化 | 剔除项 |\n");
    md.push_str("|---|---:|---:|---:|---:|---:|---|---:|\n");

    for key in SOURCES.iter().map(|c| c.key).chain(std::iter::once(LOWRATE_OVERRIDE_KEY)) {
        let Some(stat) = manifest.categories.get(key) else { continue };
        let prev_total = baseline.and_then(|b| b.categories.get(key)).map(|s| s.total);
        let delta = format_delta(prev_total, stat.total);
        let excl = stat.exclude_suffixes + stat.exclude_domains;
        let _ = writeln!(
            md,
            "| `{key}` | {} | {} | {} | {} | {} | {delta} | {excl} |",
            stat.suffixes, stat.domains, stat.keywords, stat.ips, stat.total
        );
    }

    md.push_str("\n### 远程源拉取状态\n\n");
    md.push_str("| 远程源 | 条数 | 较基线 | 状态 |\n|---|---:|---|---|\n");
    for report in reports {
        let name = report.url.rsplit('/').next().unwrap_or(report.url);
        let source_key = format!("remote:{}", report.url);
        let prev = baseline.and_then(|b| b.sources.get(&source_key).copied());
        let delta = format_delta(prev, report.rules.len());
        let status = if let Some(err) = &report.error {
            format!("失败: {}", err.replace('|', "\\|"))
        } else if report.rules.is_empty() {
            "异常: 解析结果 0 条".to_string()
        } else if report.fell_back {
            format!("已回退镜像 ({})", report.used_url)
        } else {
            "正常".to_string()
        };
        let _ = writeln!(md, "| `{name}` | {} | {delta} | {status} |", report.rules.len());
    }

    if !fetch_errors.is_empty() {
        md.push_str("\n### 拉取失败项 (阻断发布)\n\n");
        for err in fetch_errors {
            let _ = writeln!(md, "- {err}");
        }
    }
    if !drop_violations.is_empty() {
        let heading = if allow_drop { "降幅阈值告警 (已配置 allow-drop 放行)" } else { "降幅阈值拦截 (阻断发布)" };
        let _ = writeln!(md, "\n### {heading}\n");
        for msg in drop_violations {
            let _ = writeln!(md, "- {msg}");
        }
    }
    md.push('\n');
    append_env_file("GITHUB_STEP_SUMMARY", &md)
}

pub fn run(root: &Path, options: &BuildOptions) -> Result<ExitCode> {
    println!("[*] 开始拉取与解构全量规则源...");
    let baseline = load_baseline(root, options)?;

    let mut urls: Vec<&'static str> = Vec::new();
    for category in SOURCES {
        for &url in category.remote.iter().chain(category.override_direct) {
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    println!("[*] 并发拉取 {} 个远程源...", urls.len());
    let reports = parallel_map(&urls, 8, |&url| fetch_rules(url));

    let mut fetch_errors: Vec<String> = Vec::new();
    let mut fetched: HashMap<&'static str, &[Rule]> = HashMap::new();
    let mut manifest = BuildManifest { version: 1, ..Default::default() };

    for report in &reports {
        manifest.sources.insert(format!("remote:{}", report.url), report.rules.len());
        fetched.insert(report.url, report.rules.as_slice());
        if let Some(err) = &report.error {
            fetch_errors.push(format!("远程源拉取失败: {} -> {err}", report.url));
        } else if report.rules.is_empty() {
            fetch_errors.push(format!("远程源解析为空 (0 条): {}", report.url));
        } else if report.fell_back {
            let detail = report.primary_error.as_deref().unwrap_or("unknown");
            gh_warning(
                "远程源主地址失败已回退镜像",
                &format!("{} ({detail}) -> {}", report.url, report.used_url),
            );
        }
    }

    let mut categories_rules: Vec<Vec<Rule>> = Vec::with_capacity(SOURCES.len());
    let mut lowrate_authoritative_domains: HashSet<String> = HashSet::new();

    for category in SOURCES {
        println!("\n[+] 处理分类: {} ({})", category.name, category.key);
        let mut rules = Vec::new();

        // 1. 本地自定义规则
        for path in category.custom {
            let local = load_local_file(root, path)?;
            manifest.sources.insert(format!("custom:{path}"), local.len());
            println!("  -> 本地规则: {path} ({} 条)", local.len());
            if category.key == "lowrate" {
                lowrate_authoritative_domains
                    .extend(local.iter().filter(|(k, _)| k.is_domain()).map(|(_, v)| v.clone()));
            }
            rules.extend(local);
        }
        // 2. 上游规则
        for &url in category.remote {
            let remote = fetched[url];
            println!("  -> 远程源: {} ({} 条)", url.rsplit('/').next().unwrap_or(url), remote.len());
            if category.key == "lowrate" && !LOWRATE_CATCHALL_REMOTE.contains(&url) {
                lowrate_authoritative_domains
                    .extend(remote.iter().filter(|(k, _)| k.is_domain()).map(|(_, v)| v.clone()));
            }
            rules.extend(remote.iter().cloned());
        }
        // 3. 内置 Gins-Rules 源
        for &reference in category.gins {
            let gins = load_gins_rule(root, reference)?;
            manifest.sources.insert(format!("gins:{reference}"), gins.len());
            println!("  -> Gins 源: {reference} ({} 条)", gins.len());
            if gins.is_empty() {
                fetch_errors.push(format!("Gins 源为空或缺失: {reference}"));
            }
            if category.key == "lowrate" && !LOWRATE_CATCHALL_GINS.contains(&reference) {
                lowrate_authoritative_domains
                    .extend(gins.iter().filter(|(k, _)| k.is_domain()).map(|(_, v)| v.clone()));
            }
            rules.extend(gins);
        }
        categories_rules.push(rules);
    }

    println!("\n[*] 正在进行全量规则去重与跨生态分析...");

    let custom_domains = |relative: &str| -> Result<HashSet<String>> {
        Ok(load_local_file(root, relative)?
            .into_iter()
            .filter(|(kind, _)| kind.is_domain())
            .map(|(_, value)| value)
            .collect())
    };
    let apac_custom = custom_domains("custom/apac_custom.txt")?;
    let direct_custom = custom_domains("custom/direct_custom.txt")?;
    let lowrate_custom = custom_domains("custom/lowrate_custom.txt")?;
    let lowrate_custom_rules = load_local_file(root, "custom/lowrate_custom.txt")?;

    // override_direct 中的域名（如 AWS 国际站与中国区）强制归属对应分类，严禁被 direct 国内大盘吞掉
    // 但排除公共 CDN 父域 (如 cloudfront.net)
    let mut override_domains: HashMap<&str, &'static str> = HashMap::new();
    for category in SOURCES {
        for &url in category.override_direct {
            for (kind, value) in fetched[url] {
                if kind.is_domain() && !is_ai_excluded_parent(value) {
                    override_domains.insert(value, category.key);
                }
            }
        }
    }
    let override_regexes = OVERRIDE_DIRECT_PATTERNS
        .iter()
        .map(|(pattern, category)| Ok((Regex::new(pattern)?, *category)))
        .collect::<Result<Vec<(Regex, &'static str)>>>()?;

    let override_category = |domain: &str| -> Option<&'static str> {
        domain_suffixes(domain).find_map(|suffix| override_domains.get(suffix).copied()).or_else(|| {
            override_regexes.iter().find(|(regex, _)| regex.is_match(domain)).map(|(_, category)| *category)
        })
    };
    let is_youtube_or_streaming = |domain: &str| {
        lowrate_custom.contains(domain)
            || domain.contains("youtube")
            || domain.contains("googlevideo")
            || domain.contains("ytimg")
            || domain.ends_with("youtu.be")
    };
    let is_apple_proxy_service = |domain: &str| {
        if direct_custom.contains(domain) {
            return false;
        }
        lowrate_custom.contains(domain) || APPLE_PROXY_SERVICES.contains(&domain) || domain.ends_with(".itunes.apple.com")
    };

    // 跨分类冲突消除优先级 (SOURCES 顺序): direct -> apac -> ai -> lowrate
    let mut seen_domains: HashSet<String> = HashSet::new();
    let mut seen_ips: HashSet<String> = HashSet::new();
    let mut forced_moves: HashMap<&str, Vec<Rule>> = SOURCES.iter().map(|category| (category.key, Vec::new())).collect();
    let mut buckets: Vec<(&'static str, Bucket)> = Vec::with_capacity(SOURCES.len() + 1);

    for (category, raw_rules) in SOURCES.iter().zip(&categories_rules) {
        let key = category.key;
        let moved = forced_moves.get_mut(key).map(std::mem::take).unwrap_or_default();
        let mut filtered: Vec<Rule> = Vec::new();

        for (kind, value) in raw_rules.iter().chain(&moved) {
            let is_domain = kind.is_domain();
            if key == "direct"
                && is_domain
                && (apac_custom.contains(value)
                    || value.contains("github")
                    || is_apple_proxy_service(value)
                    || is_youtube_or_streaming(value))
            {
                // 排除上游误标为 direct 的 Steam 商城/社区、GitHub、流媒体，以及 Apple 音乐云同步与 Storefront 服务
                continue;
            }
            if is_domain && matches!(key, "direct" | "apac") {
                if let Some(forced) = override_category(value).filter(|forced| *forced != key) {
                    // AWS 等强制分流域名不得落入 direct / apac，转交给目标分类 (排除 GitHub/流媒体与公共 CDN)
                    if !(forced == "ai"
                        && (is_ai_excluded_parent(value)
                            || value.contains("github")
                            || is_youtube_or_streaming(value)
                            || is_apple_proxy_service(value)))
                    {
                        forced_moves
                            .get_mut(forced)
                            .ok_or_else(|| format!("未知的强制分流目标分类: {forced}"))?
                            .push((*kind, value.clone()));
                    }
                    continue;
                }
            }
            if matches!(key, "apac" | "ai" | "lowrate") && is_domain && direct_custom.contains(value) {
                // 排除上游误标为 proxy 的游戏下载 CDN 域名与 Apple 直连/下载 CDN
                continue;
            }
            if key == "ai"
                && is_domain
                && (is_youtube_or_streaming(value) || is_ai_excluded_parent(value) || value.contains("github"))
            {
                // 排除混入 ai 的 YouTube/流媒体、公共 CloudFront CDN 与 GitHub S3 域名，留给 lowrate 组
                continue;
            }

            let first_seen = match kind {
                Kind::DomainSuffix | Kind::Domain => seen_domains.insert(value.clone()),
                Kind::IpCidr => seen_ips.insert(value.clone()),
                Kind::Keyword => true,
            };
            if first_seen {
                filtered.push((*kind, value.clone()));
            }
        }

        buckets.push((key, deduplicate(&filtered)));
    }

    // 计算 ai 剔除集合与跨平台 lowrate_override 规则集
    let ai_idx = buckets.iter().position(|(k, _)| *k == "ai").expect("ai category exists");
    let lowrate_idx = buckets.iter().position(|(k, _)| *k == "lowrate").expect("lowrate category exists");
    let lowrate_snapshot = buckets[lowrate_idx].1.clone();
    let override_bucket = compute_lowrate_overrides(
        &mut buckets[ai_idx].1,
        &lowrate_snapshot,
        &lowrate_custom_rules,
        &lowrate_authoritative_domains,
    );
    buckets.push((LOWRATE_OVERRIDE_KEY, override_bucket));

    for (key, bucket) in &buckets {
        let stats = CategoryStats::from_bucket(bucket);
        println!(
            "  [{key:<16}] 域名后缀: {:>6} | 完整域名: {:>5} | 关键字: {:>3} | IP: {:>5}",
            stats.suffixes, stats.domains, stats.keywords, stats.ips
        );
        if !bucket.exclude_suffixes.is_empty() || !bucket.exclude_domains.is_empty() {
            println!(
                "  [{key:<16}] sing-box 剔除 / lowrate_override: {} 条 (后缀 {} / 完整 {})",
                bucket.exclude_suffixes.len() + bucket.exclude_domains.len(),
                bucket.exclude_suffixes.len(),
                bucket.exclude_domains.len()
            );
        }
        manifest.categories.insert((*key).to_string(), stats);
    }

    // 基线降幅校验
    let drop_violations = baseline
        .as_ref()
        .map(|base| validate_against_baseline(base, &manifest, options.max_drop))
        .unwrap_or_default();

    write_build_summary(&reports, &manifest, baseline.as_ref(), &fetch_errors, &drop_violations, options.allow_drop)?;

    let mut failed = false;
    for err in &fetch_errors {
        eprintln!("[ERROR] {err}");
        gh_error("规则源拉取失败", err);
        failed = true;
    }
    for msg in &drop_violations {
        if options.allow_drop {
            println!("[WARN] (已通过 --allow-drop 放行) {msg}");
            gh_warning("规则条数降幅超阈值 (已放行)", msg);
        } else {
            eprintln!("[ERROR] {msg}");
            gh_error("规则条数异常下降", msg);
            failed = true;
        }
    }
    if failed {
        eprintln!("\n[ERROR] 规则构建校验未通过，已中止导出与发布！");
        return Ok(ExitCode::FAILURE);
    }

    // 全部校验通过后再导出规则与 manifest.json
    for (key, bucket) in &buckets {
        export::write_all(root, key, bucket)?;
    }
    let manifest_path = root.join("rules").join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)? + "\n")?;

    println!("\n[*] 规则文本导出完毕！目标路径: {}", root.join("rules").display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_domain_suffixes() {
        let suffixes: Vec<_> = domain_suffixes("a.b.c").collect();
        assert_eq!(suffixes, vec!["a.b.c", "b.c", "c"]);
    }

    #[test]
    fn drops_domains_covered_by_suffix() {
        let rules = vec![
            (Kind::DomainSuffix, "example.com".to_string()),
            (Kind::Domain, "www.example.com".to_string()),
            (Kind::Domain, "example.com".to_string()),
            (Kind::Domain, "other.org".to_string()),
        ];
        let bucket = deduplicate(&rules);
        assert_eq!(bucket.suffixes, vec!["example.com"]);
        assert_eq!(bucket.domains, vec!["example.com", "other.org"]);
    }

    #[test]
    fn detects_gins_force_proxy() {
        assert!(is_gins_force_proxy("ipinfo.io"));
        assert!(is_gins_force_proxy("api.ipinfo.io"));
        assert!(is_gins_force_proxy("v16.tiktokcdn-us.com"));
        assert!(!is_gins_force_proxy("myipinfo.io"));
    }

    #[test]
    fn classifies_shared_cloud_tenants_safely() {
        assert!(is_shared_cloud_tenant_subdomain("s3.us-east-1.amazonaws.com"));
        assert!(is_shared_cloud_tenant_subdomain("s3-ap-northeast-1.amazonaws.com"));
        assert!(is_shared_cloud_tenant_subdomain("github-cloud.s3.amazonaws.com"));
        assert!(is_shared_cloud_tenant_subdomain("eset-prod.elb.eu-central-1.amazonaws.com"));
        assert!(is_shared_cloud_tenant_subdomain("discord-attachments.storage.googleapis.com"));
        assert!(is_shared_cloud_tenant_subdomain("abc.awsglobalaccelerator.com"));
        assert!(is_shared_cloud_tenant_subdomain("app.eu-west-1.elasticbeanstalk.com"));

        // 绝不误判 AWS 区域根域名或核心服务端点
        assert!(!is_shared_cloud_tenant_subdomain("us-west-2.amazonaws.com"));
        assert!(!is_shared_cloud_tenant_subdomain("ec2.us-west-2.amazonaws.com"));
        assert!(!is_shared_cloud_tenant_subdomain("bedrock-runtime.us-east-1.amazonaws.com"));
        assert!(!is_shared_cloud_tenant_subdomain("files.oaiusercontent.com"));
    }

    #[test]
    fn computes_lowrate_overrides_without_leaking_real_ai_subdomains() {
        let mut ai = deduplicate(&[
            (Kind::DomainSuffix, "google.com".into()),
            (Kind::DomainSuffix, "googleapis.com".into()),
            (Kind::DomainSuffix, "amazonaws.com".into()),
            (Kind::DomainSuffix, "oaiusercontent.com".into()),
            (Kind::Keyword, "colab".into()),
        ]);
        let lowrate = deduplicate(&[
            (Kind::DomainSuffix, "photos.google.com".into()),
            (Kind::Domain, "youtubei.googleapis.com".into()),
            (Kind::DomainSuffix, "s3.us-east-1.amazonaws.com".into()),
            (Kind::DomainSuffix, "us-west-2.amazonaws.com".into()),
            (Kind::DomainSuffix, "files.oaiusercontent.com".into()),
            (Kind::DomainSuffix, "calicolabs.com".into()),
            (Kind::DomainSuffix, "netflix.com".into()),
        ]);
        let custom = vec![
            (Kind::DomainSuffix, "photos.google.com".into()),
            (Kind::Domain, "youtubei.googleapis.com".into()),
        ];
        let auth: HashSet<String> = ["photos.google.com", "youtubei.googleapis.com", "us-west-2.amazonaws.com"]
            .into_iter()
            .map(String::from)
            .collect();

        let overrides = compute_lowrate_overrides(&mut ai, &lowrate, &custom, &auth);
        assert_eq!(overrides.suffixes, vec!["calicolabs.com", "photos.google.com", "s3.us-east-1.amazonaws.com"]);
        assert_eq!(overrides.domains, vec!["youtubei.googleapis.com"]);
        assert_eq!(ai.exclude_suffixes, overrides.suffixes);
        assert_eq!(ai.exclude_domains, overrides.domains);
    }

    #[test]
    fn detects_threshold_drop_violations() {
        let mut prev = BuildManifest { version: 1, ..Default::default() };
        prev.sources.insert("remote:aws".into(), 100);
        prev.categories.insert(
            "ai".into(),
            CategoryStats { suffixes: 1000, domains: 20, keywords: 5, ips: 500, total: 1525, ..Default::default() },
        );

        let mut curr = prev.clone();
        curr.sources.insert("remote:aws".into(), 50); // -50% > 30%
        curr.categories.insert(
            "ai".into(),
            CategoryStats { suffixes: 600, domains: 20, keywords: 5, ips: 500, total: 1125, ..Default::default() },
        );

        let violations = validate_against_baseline(&prev, &curr, 0.30);
        assert!(violations.iter().any(|v| v.contains("remote:aws")));
        assert!(violations.iter().any(|v| v.contains("分类 ai 域名条数")));
    }
}
