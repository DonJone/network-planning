//! 规则集主构建流程
//! 负责拉取、解构、清洗、冲突判定、合并去重，并生成 Mihomo、Loon、Shadowrocket、Sing-box 的四大块规则文件。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use regex::Regex;

use crate::Result;
use crate::config::{GINS_FORCE_PROXY, OVERRIDE_DIRECT_PATTERNS, SOURCES};
use crate::export::{self, Bucket};
use crate::fetch::{fetch_text, parallel_map};
use crate::parser::{Kind, Rule, file_lines, parse_gins_line, parse_line, splitlines};

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

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

fn fetch_rules(url: &str) -> Vec<Rule> {
    match fetch_text(url, USER_AGENT, 3, Duration::from_secs(25), Duration::from_secs(1)) {
        Ok(text) => splitlines(&text).into_iter().filter_map(parse_line).collect(),
        Err(err) => {
            println!("  [WARN] 拉取最终失败: {url} -> {err}");
            Vec::new()
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

/// 找出被本分类父级后缀/关键字覆盖、但自定义规则要求归属其他分类的域名，供二进制规则剔除
/// 例：ai 含 `google.com` 后缀，lowrate_custom 的 `photos.google.com` 需从 ai 中剔除才能落到 lowrate
fn set_exclusions(bucket: &mut Bucket, preferred: &[Rule]) {
    let suffixes: HashSet<&str> = bucket.suffixes.iter().map(String::as_str).collect();
    let covered = |domain: &str| {
        domain_suffixes(domain).any(|parent| suffixes.contains(parent))
            || bucket.keywords.iter().any(|keyword| domain.contains(keyword.as_str()))
    };
    let mut exclude_suffixes = BTreeSet::new();
    let mut exclude_domains = BTreeSet::new();
    for (kind, value) in preferred {
        match kind {
            Kind::DomainSuffix if covered(value) => exclude_suffixes.insert(value.clone()),
            Kind::Domain if covered(value) => exclude_domains.insert(value.clone()),
            _ => false,
        };
    }
    bucket.exclude_suffixes = exclude_suffixes.into_iter().collect();
    bucket.exclude_domains = exclude_domains.into_iter().collect();
}

pub fn run(root: &Path) -> Result<ExitCode> {
    println!("[*] 开始拉取与解构全量规则源...");

    let mut urls: Vec<&str> = Vec::new();
    for category in SOURCES {
        for url in category.remote.iter().chain(category.override_direct) {
            if !urls.contains(url) {
                urls.push(url);
            }
        }
    }
    println!("[*] 并发拉取 {} 个远程源...", urls.len());
    let fetched: HashMap<&str, Vec<Rule>> =
        urls.iter().copied().zip(parallel_map(&urls, 8, |url| fetch_rules(url))).collect();

    let mut categories_rules: Vec<Vec<Rule>> = Vec::with_capacity(SOURCES.len());
    for category in SOURCES {
        println!("\n[+] 处理分类: {} ({})", category.name, category.key);
        let mut rules = Vec::new();

        // 1. 本地自定义规则
        for path in category.custom {
            let local = load_local_file(root, path)?;
            println!("  -> 本地规则: {path} ({} 条)", local.len());
            rules.extend(local);
        }
        // 2. 上游规则
        for url in category.remote {
            let remote = &fetched[*url];
            println!("  -> 远程源: {} ({} 条)", url.rsplit('/').next().unwrap_or(url), remote.len());
            rules.extend(remote.iter().cloned());
        }
        // 3. 内置 Gins-Rules 源
        for reference in category.gins {
            let gins = load_gins_rule(root, reference)?;
            println!("  -> Gins 源: {reference} ({} 条)", gins.len());
            rules.extend(gins);
        }
        categories_rules.push(rules);
    }

    println!("\n[*] 正在进行全量规则去重与跨生态导出...");

    // 自定义规则具备最高权威性：
    // 1. apac_custom 中的域名（如 Steam 商城/社区、Epic、Telegram）严禁被 direct 误吞
    // 2. direct_custom 中的域名（如 Steam 下载 steamcontent、Epic 下载）严禁被 proxy/lowrate 误吞
    // 3. lowrate_custom 及流媒体（YouTube、Google 相册/网盘、Emby、GitHub）严禁被 ai 中的 google.list 误吞
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

    // 4. override_direct 中的域名（如 AWS 国际站与中国区）强制归属对应分类，严禁被 direct 国内大盘吞掉
    let mut override_domains: HashMap<&str, &'static str> = HashMap::new();
    for category in SOURCES {
        for url in category.override_direct {
            for (kind, value) in &fetched[*url] {
                if kind.is_domain() {
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

    // 跨分类冲突消除优先级 (SOURCES 顺序)：
    // 1. direct  (直连白名单与国内大盘最高保护，但排除明确归属于 apac 的商城与社区域名，以及 Apple 海外Storefront/Music云同步)
    // 2. apac    (专属即时通讯 Telegram/X 与 Steam/Epic 商城/社区)
    // 3. ai      (专属 AI 模型与 Google 核心，排除流媒体与 YouTube)
    // 4. lowrate (专属流媒体与大吞吐，最后承接通用海外 proxy 兜底)
    let mut seen_domains: HashSet<String> = HashSet::new();
    let mut seen_ips: HashSet<String> = HashSet::new();
    let mut forced_moves: HashMap<&str, Vec<Rule>> = SOURCES.iter().map(|category| (category.key, Vec::new())).collect();

    for (category, raw_rules) in SOURCES.iter().zip(&categories_rules) {
        let key = category.key;
        let moved = forced_moves.get_mut(key).map(std::mem::take).unwrap_or_default();
        let mut filtered: Vec<Rule> = Vec::new();

        for (kind, value) in raw_rules.iter().chain(&moved) {
            let is_domain = kind.is_domain();
            if is_domain && matches!(key, "direct" | "apac") {
                if let Some(forced) = override_category(value).filter(|forced| *forced != key) {
                    // AWS 等强制分流域名不得落入 direct / apac，转交给目标分类
                    forced_moves
                        .get_mut(forced)
                        .ok_or_else(|| format!("未知的强制分流目标分类: {forced}"))?
                        .push((*kind, value.clone()));
                    continue;
                }
            }
            if key == "direct"
                && is_domain
                && (apac_custom.contains(value) || value.contains("github") || is_apple_proxy_service(value))
            {
                // 排除上游误标为 direct 的 Steam 商城/社区、GitHub，以及 Apple 音乐云同步与 Storefront 切换服务 (留给 lowrate 组)
                continue;
            }
            if matches!(key, "apac" | "ai" | "lowrate") && is_domain && direct_custom.contains(value) {
                // 排除上游误标为 proxy 的游戏下载 CDN 域名与 Apple 直连/下载 CDN
                continue;
            }
            if key == "ai" && is_domain && is_youtube_or_streaming(value) {
                // 排除上游 google.list 中混杂的 YouTube/流媒体域名，留给 lowrate 组
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

        let mut bucket = deduplicate(&filtered);
        if key == "ai" {
            // lowrate_custom 中被 ai 父级后缀 (google.com / googleapis.com 等) 覆盖的域名，在二进制规则中剔除
            set_exclusions(&mut bucket, &lowrate_custom_rules);
        }
        println!(
            "  [{key:<7}] 域名后缀: {:>6} | 完整域名: {:>5} | 关键字: {:>3} | IP: {:>5}",
            bucket.suffixes.len(),
            bucket.domains.len(),
            bucket.keywords.len(),
            bucket.ips.len()
        );
        if !bucket.exclude_suffixes.is_empty() || !bucket.exclude_domains.is_empty() {
            println!(
                "  [{key:<7}] sing-box 二进制剔除: {}",
                bucket.exclude_suffixes.iter().chain(&bucket.exclude_domains).cloned().collect::<Vec<_>>().join(", ")
            );
        }

        // 导出 4 大生态
        export::write_all(root, key, &bucket)?;
    }

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
    fn excludes_preferred_domains_covered_by_parent_suffix() {
        let mut bucket = deduplicate(&[
            (Kind::DomainSuffix, "google.com".to_string()),
            (Kind::DomainSuffix, "googleapis.com".to_string()),
        ]);
        let preferred = vec![
            (Kind::DomainSuffix, "photos.google.com".to_string()),
            (Kind::Domain, "youtubei.googleapis.com".to_string()),
            (Kind::DomainSuffix, "netflix.com".to_string()),
            (Kind::Keyword, "youtube".to_string()),
        ];
        set_exclusions(&mut bucket, &preferred);
        assert_eq!(bucket.exclude_suffixes, vec!["photos.google.com"]);
        assert_eq!(bucket.exclude_domains, vec!["youtubei.googleapis.com"]);
    }
}
