//! Gins-Rules CDN 快照同步
//! 拉取 config 中 gins 字段引用的 CDN 规则 (rules.ichimarugin728.dev)，
//! 归一化排序后写入 gins-rules/cdn/shadowrocket/，仅在内容变化时落盘，并输出新增/删除统计。
//! 拉取失败时保留原快照，不会清空文件。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use crate::Result;
use crate::config::{GINS_CDN_BASE, SOURCES};
use crate::fetch::{append_env_file, fetch_text, gh_warning, parallel_map};
use crate::parser::{is_blank_or_comment, splitlines, strip};

const USER_AGENT: &str = "network-planning-gins-cdn-sync";

enum SyncOutcome {
    Updated { total: usize, added: usize, removed: usize },
    Failed { url: String, reason: String },
}

fn cdn_path(reference: &str) -> String {
    match reference.split_once('/') {
        Some(("asn", name)) => format!("ip/{name}"),
        _ => reference.to_string(),
    }
}

fn normalize(text: &str) -> BTreeSet<String> {
    splitlines(text).into_iter().map(strip).filter(|line| !is_blank_or_comment(line)).map(str::to_owned).collect()
}

fn sync(cdn_dir: &Path, reference: &str) -> Result<SyncOutcome> {
    let path = cdn_path(reference);
    let url = format!("{GINS_CDN_BASE}/{path}.list");
    let new = match fetch_text(&url, USER_AGENT, 3, Duration::from_secs(30), Duration::from_secs(2)) {
        Ok(text) => normalize(&text),
        Err(err) => {
            println!("  [WARN] 拉取失败: {url} -> {err}");
            return Ok(SyncOutcome::Failed { url, reason: err });
        }
    };
    if new.is_empty() {
        let reason = "返回内容为空".to_string();
        println!("  [WARN] 拉取失败: {url} -> {reason}");
        return Ok(SyncOutcome::Failed { url, reason });
    }

    let snapshot = cdn_dir.join(format!("{path}.list"));
    let old = if snapshot.exists() { normalize(&fs::read_to_string(&snapshot)?) } else { BTreeSet::new() };

    let added = new.difference(&old).count();
    let removed = old.difference(&new).count();
    if new != old {
        if let Some(parent) = snapshot.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut content = new.iter().map(String::as_str).collect::<Vec<_>>().join("\n");
        content.push('\n');
        fs::write(&snapshot, content)?;
    }
    Ok(SyncOutcome::Updated { total: new.len(), added, removed })
}

pub fn run(root: &Path) -> Result<ExitCode> {
    let mut refs: Vec<&str> = Vec::new();
    for reference in SOURCES.iter().flat_map(|category| category.gins) {
        if !refs.contains(reference) {
            refs.push(reference);
        }
    }
    println!("[*] 同步 Gins-Rules CDN 快照 ({} 个规则集)...", refs.len());

    let cdn_dir = root.join("gins-rules").join("cdn").join("shadowrocket");
    let results = parallel_map(&refs, 8, |reference| sync(&cdn_dir, reference));

    let mut rows = Vec::new();
    let mut changed = false;
    let mut failed = 0;
    for (reference, result) in refs.iter().zip(results) {
        match result? {
            SyncOutcome::Failed { url, reason } => {
                failed += 1;
                gh_warning("Gins CDN 快照拉取失败", &format!("{reference} ({url}): {reason}，保留原快照"));
                let safe_reason = reason.replace('|', "\\|");
                rows.push(format!("| {reference} | - | - | - | 拉取失败，保留原快照 ({safe_reason}) |"));
                println!("  [FAIL ] {reference:<20} 拉取失败，保留原快照");
            }
            SyncOutcome::Updated { total, added, removed } => {
                let diff = added > 0 || removed > 0;
                changed |= diff;
                let status = if diff { "有变更" } else { "无变化" };
                rows.push(format!("| {reference} | {total} | +{added} | -{removed} | {status} |"));
                let tag = if diff { "DIFF " } else { "SAME " };
                println!("  [{tag}] {reference:<20} 共 {total:>7} 条  +{added:<6} -{removed:<6}");
            }
        }
    }

    let failed_note = if failed > 0 { format!("，{failed} 个拉取失败") } else { String::new() };
    println!("\n[*] CDN 快照{}{failed_note}", if changed { "有变更" } else { "无变化" });

    append_env_file(
        "GITHUB_STEP_SUMMARY",
        &format!(
            "## Gins-Rules CDN 快照\n\n| 规则集 | 条数 | 新增 | 删除 | 状态 |\n|---|---|---|---|---|\n{}\n\n",
            rows.join("\n")
        ),
    )?;
    append_env_file("GITHUB_OUTPUT", &format!("changed={changed}\nfailed={failed}\n"))?;

    Ok(if failed == refs.len() { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}
