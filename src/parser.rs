//! 统一规则解析器
//! 将不同来源（Clash/Mihomo、Loon、Shadowrocket、Sing-box、MetaCubeX、Gins-Rules）的文本规则归一化为标准内部结构。

use std::net::{IpAddr, Ipv4Addr};
use std::sync::LazyLock;

use ipnet::IpNet;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    DomainSuffix,
    Domain,
    Keyword,
    IpCidr,
}

impl Kind {
    pub fn is_domain(self) -> bool {
        matches!(self, Kind::DomainSuffix | Kind::Domain)
    }
}

pub type Rule = (Kind, String);

static DOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9_\-\.]+\.[a-z0-9_\-]+$").expect("valid domain regex"));

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("//") || line.starts_with(';')
}

/// 解析一行 Clash/Loon/Shadowrocket/MetaCubeX 风格规则
pub fn parse_line(line: &str) -> Option<Rule> {
    let line = strip(line);
    if line.is_empty() || is_comment(line) {
        return None;
    }

    // 去除行内多余标记 (如 ,no-resolve, ,DIRECT 等策略标识)
    let parts: Vec<&str> = line.split(',').map(strip).filter(|part| !part.is_empty()).collect();
    let first = *parts.first()?;

    // 1. 标准 Shadowrocket / Loon / Clash 前缀
    if parts.len() >= 2 {
        let kind = match first.to_uppercase().as_str() {
            "DOMAIN-SUFFIX" => Some(Kind::DomainSuffix),
            "DOMAIN-KEYWORD" => Some(Kind::Keyword),
            "DOMAIN" => Some(Kind::Domain),
            "IP-CIDR" | "IP-CIDR6" => Some(Kind::IpCidr),
            _ => None,
        };
        if let Some(kind) = kind {
            return Some((kind, parts[1].to_lowercase()));
        }
    }

    // 2. MetaCubeX / V2Ray 常用前缀
    if first.starts_with("+.") || first.starts_with('.') {
        let domain = first.trim_start_matches(['+', '.']).to_lowercase();
        if !domain.is_empty() {
            return Some((Kind::DomainSuffix, domain));
        }
    } else if first.starts_with('+') {
        let domain = first.trim_start_matches('+').to_lowercase();
        if !domain.is_empty() {
            return Some((Kind::DomainSuffix, domain));
        }
    } else if let Some(rest) = first.strip_prefix("full:") {
        if !rest.is_empty() {
            return Some((Kind::Domain, rest.to_lowercase()));
        }
    } else if let Some(rest) = first.strip_prefix("keyword:") {
        if !rest.is_empty() {
            return Some((Kind::Keyword, rest.to_lowercase()));
        }
    }

    // 3. 纯 IP-CIDR 或单个 IP
    if let Some(network) = parse_network(first) {
        return Some((Kind::IpCidr, network));
    }

    // 4. 看起来像域名（包含至少一个点，且不含非法字符）
    let candidate = first.to_lowercase();
    if DOMAIN_RE.is_match(&candidate) {
        return Some((Kind::DomainSuffix, candidate));
    }

    None
}

/// 按 gins-rules/cmd/compile 的 parseSource 语义解析 Gins 源文本行
pub fn parse_gins_line(line: &str) -> Option<Rule> {
    let line = strip(line);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    if let Some(rest) = line.strip_prefix("full:") {
        return parse_line(&format!("DOMAIN,{rest}"));
    }
    if let Some(rest) = line.strip_prefix("keyword:") {
        return parse_line(&format!("DOMAIN-KEYWORD,{rest}"));
    }
    if ["regexp:", "process:", "user-agent:", "asn:"].iter().any(|prefix| line.starts_with(prefix)) {
        return None;
    }
    if line.contains('/') {
        return parse_line(&format!("IP-CIDR,{line}"));
    }
    let domain = line.strip_prefix("+.").unwrap_or(line);
    let domain = domain.strip_prefix('.').unwrap_or(domain);
    parse_line(&format!("DOMAIN-SUFFIX,{domain}"))
}

/// 解析 IP / CIDR 并规范化为网络地址 (等价于 Python `ipaddress.ip_network(strict=False)`)
fn parse_network(text: &str) -> Option<String> {
    let (address, prefix) = match text.split_once('/') {
        Some((address, prefix)) => (address, Some(prefix)),
        None => (text, None),
    };
    let address: IpAddr = address.parse().ok()?;
    let max_len: u32 = if address.is_ipv4() { 32 } else { 128 };
    let prefix_len = match prefix {
        None => max_len,
        Some(prefix) if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit()) => {
            let len: u32 = prefix.parse().ok()?;
            if len > max_len {
                return None;
            }
            len
        }
        Some(mask) if address.is_ipv4() => mask_prefix_len(mask)?,
        Some(_) => return None,
    };
    let network = IpNet::new(address, u8::try_from(prefix_len).ok()?).ok()?;
    Some(network.trunc().to_string())
}

/// 将 IPv4 子网掩码或反掩码 (如 255.255.255.0 / 0.0.0.255) 转为前缀长度
fn mask_prefix_len(mask: &str) -> Option<u32> {
    let value = u32::from(mask.parse::<Ipv4Addr>().ok()?);
    [value, !value].into_iter().find_map(|bits| {
        let ones = bits.leading_ones();
        (bits.checked_shl(ones).unwrap_or(0) == 0).then_some(ones)
    })
}

/// 等价于 Python `str.strip()`
pub fn strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// 等价于 Python `str.splitlines()`
pub fn splitlines(text: &str) -> Vec<&str> {
    split_lines_by(text, |c| {
        matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}')
    })
}

/// 等价于 Python 文本模式逐行读取 (通用换行符 \n、\r、\r\n)
pub fn file_lines(text: &str) -> Vec<&str> {
    split_lines_by(text, |c| matches!(c, '\n' | '\r'))
}

fn split_lines_by(text: &str, is_break: impl Fn(char) -> bool) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_break(c) {
            continue;
        }
        lines.push(&text[start..index]);
        start = index + c.len_utf8();
        if c == '\r' {
            if let Some(&(next, '\n')) = chars.peek() {
                chars.next();
                start = next + 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// 是否为可忽略的空行或注释行 (CDN 快照归一化使用)
pub fn is_blank_or_comment(line: &str) -> bool {
    line.is_empty() || is_comment(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: Kind, value: &str) -> Option<Rule> {
        Some((kind, value.to_string()))
    }

    #[test]
    fn parses_prefixed_rules() {
        assert_eq!(parse_line("DOMAIN-SUFFIX,Example.COM,DIRECT"), rule(Kind::DomainSuffix, "example.com"));
        assert_eq!(parse_line("domain,a.b.com"), rule(Kind::Domain, "a.b.com"));
        assert_eq!(parse_line("DOMAIN-KEYWORD,google"), rule(Kind::Keyword, "google"));
        assert_eq!(parse_line("IP-CIDR6,2001:DB8::/32,no-resolve"), rule(Kind::IpCidr, "2001:db8::/32"));
        assert_eq!(parse_line("IP-CIDR,1.2.3.4/24"), rule(Kind::IpCidr, "1.2.3.4/24"));
    }

    #[test]
    fn parses_metacubex_rules() {
        assert_eq!(parse_line("+.Example.com"), rule(Kind::DomainSuffix, "example.com"));
        assert_eq!(parse_line(".example.com"), rule(Kind::DomainSuffix, "example.com"));
        assert_eq!(parse_line("full:www.example.com"), rule(Kind::Domain, "www.example.com"));
        assert_eq!(parse_line("keyword:tiktok"), rule(Kind::Keyword, "tiktok"));
        assert_eq!(parse_line("example.com"), rule(Kind::DomainSuffix, "example.com"));
    }

    #[test]
    fn normalizes_networks() {
        assert_eq!(parse_line("1.2.3.4"), rule(Kind::IpCidr, "1.2.3.4/32"));
        assert_eq!(parse_line("1.2.3.4/24"), rule(Kind::IpCidr, "1.2.3.0/24"));
        assert_eq!(parse_line("1.2.3.4/255.255.0.0"), rule(Kind::IpCidr, "1.2.0.0/16"));
        assert_eq!(parse_line("2001:0DB8:0:0::1/64"), rule(Kind::IpCidr, "2001:db8::/64"));
        assert_eq!(parse_line("1.2.3.4/33"), None);
    }

    #[test]
    fn skips_comments_and_garbage() {
        assert_eq!(parse_line("  # comment"), None);
        assert_eq!(parse_line("// comment"), None);
        assert_eq!(parse_line("; comment"), None);
        assert_eq!(parse_line("payload:"), None);
        assert_eq!(parse_line("localhost"), None);
    }

    #[test]
    fn parses_gins_rules() {
        assert_eq!(parse_gins_line("+.example.com"), rule(Kind::DomainSuffix, "example.com"));
        assert_eq!(parse_gins_line("full:a.example.com"), rule(Kind::Domain, "a.example.com"));
        assert_eq!(parse_gins_line("keyword:steam"), rule(Kind::Keyword, "steam"));
        assert_eq!(parse_gins_line("10.0.0.1/8"), rule(Kind::IpCidr, "10.0.0.1/8"));
        assert_eq!(parse_gins_line("regexp:^a$"), None);
        assert_eq!(parse_gins_line("asn:13335"), None);
    }

    #[test]
    fn splits_lines_like_python() {
        assert_eq!(splitlines("a\r\nb\rc\nd\u{2028}e"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(file_lines("a\r\nb\x0bc\n"), vec!["a", "b\x0bc"]);
    }
}
