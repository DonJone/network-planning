use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use crate::Result;

pub struct FetchOutcome {
    pub text: String,
    pub used_url: String,
    pub fell_back: bool,
    pub primary_error: Option<String>,
}

/// 拉取远程文本，支持重试；非 UTF-8 字节直接丢弃
/// 显式声明 identity 编码：jsDelivr 对 `Accept-Encoding: gzip` 的部分路径会返回 403
pub fn fetch_text(
    url: &str,
    user_agent: &str,
    retries: u32,
    timeout: Duration,
    backoff: Duration,
) -> std::result::Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();
    let mut last_error = String::new();
    for attempt in 1..=retries {
        let body = agent
            .get(url)
            .set("User-Agent", user_agent)
            .set("Accept-Encoding", "identity")
            .call()
            .map_err(|err| err.to_string())
            .and_then(|response| {
                let mut body = Vec::new();
                response.into_reader().read_to_end(&mut body).map_err(|err| err.to_string())?;
                Ok(body)
            });
        match body {
            Ok(body) => return Ok(decode_utf8_ignore(&body)),
            Err(err) => last_error = err,
        }
        if attempt < retries {
            thread::sleep(backoff);
        }
    }
    Err(last_error)
}

/// 为 jsDelivr GitHub 代理地址生成备用回源地址 (另一节点 + raw.githubusercontent.com)
pub fn fallback_urls(url: &str) -> Vec<String> {
    let (alt_host, rest) = if let Some(rest) = url.strip_prefix("https://fastly.jsdelivr.net/gh/") {
        ("https://cdn.jsdelivr.net/gh/", rest)
    } else if let Some(rest) = url.strip_prefix("https://cdn.jsdelivr.net/gh/") {
        ("https://fastly.jsdelivr.net/gh/", rest)
    } else {
        return Vec::new();
    };

    let mut fallbacks = vec![format!("{alt_host}{rest}")];
    if let Some((owner_repo_ref, path)) = split_jsdelivr_gh(rest) {
        fallbacks.push(format!("https://raw.githubusercontent.com/{owner_repo_ref}/{path}"));
    }
    fallbacks
}

/// 解析 `<owner>/<repo>@<ref>/<path>` -> `("<owner>/<repo>/<ref>", "<path>")`
fn split_jsdelivr_gh(rest: &str) -> Option<(String, &str)> {
    let (owner, after_owner) = rest.split_once('/')?;
    let (repo_ref, path) = after_owner.split_once('/')?;
    let (repo, git_ref) = repo_ref.split_once('@')?;
    if owner.is_empty() || repo.is_empty() || git_ref.is_empty() || path.is_empty() {
        return None;
    }
    Some((format!("{owner}/{repo}/{git_ref}"), path))
}

/// 拉取远程文本，主地址失败时自动回退到备用镜像或 GitHub Raw 源
pub fn fetch_text_with_fallback(
    url: &str,
    user_agent: &str,
    retries: u32,
    timeout: Duration,
    backoff: Duration,
) -> std::result::Result<FetchOutcome, String> {
    let primary_err = match fetch_text(url, user_agent, retries, timeout, backoff) {
        Ok(text) => {
            return Ok(FetchOutcome {
                text,
                used_url: url.to_string(),
                fell_back: false,
                primary_error: None,
            });
        }
        Err(err) => err,
    };

    let fallbacks = fallback_urls(url);
    if fallbacks.is_empty() {
        return Err(primary_err);
    }

    let mut errors = vec![format!("{url}: {primary_err}")];
    for fallback in fallbacks {
        match fetch_text(&fallback, user_agent, retries, timeout, backoff) {
            Ok(text) => {
                return Ok(FetchOutcome {
                    text,
                    used_url: fallback,
                    fell_back: true,
                    primary_error: Some(primary_err),
                });
            }
            Err(err) => errors.push(format!("{fallback}: {err}")),
        }
    }
    Err(errors.join(" | "))
}

fn decode_utf8_ignore(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
    }
    text
}

/// 向环境变量指定文件追加内容 (如 GITHUB_STEP_SUMMARY / GITHUB_OUTPUT)
pub fn append_env_file(var: &str, content: &str) -> Result<()> {
    if let Some(path) = env::var_os(var) {
        OpenOptions::new().create(true).append(true).open(path)?.write_all(content.as_bytes())?;
    }
    Ok(())
}

fn escape_gh_annotation(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

/// 在 GitHub Actions 环境输出 warning 注解，使工作流摘要高亮告警
pub fn gh_warning(title: &str, message: &str) {
    if env::var_os("GITHUB_ACTIONS").is_some() {
        println!("::warning title={}::{}", escape_gh_annotation(title), escape_gh_annotation(message));
    }
}

/// 在 GitHub Actions 环境输出 error 注解
pub fn gh_error(title: &str, message: &str) {
    if env::var_os("GITHUB_ACTIONS").is_some() {
        println!("::error title={}::{}", escape_gh_annotation(title), escape_gh_annotation(message));
    }
}

/// 以固定数量的工作线程并发映射，结果保持输入顺序
pub fn parallel_map<T, R, F>(items: &[T], workers: usize, task: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..workers.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    let result = task(item);
                    results.lock().expect("results lock")[index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("results lock")
        .into_iter()
        .map(|result| result.expect("every item processed"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_jsdelivr_fallbacks() {
        let url = "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws.list";
        assert_eq!(
            fallback_urls(url),
            vec![
                "https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws.list",
                "https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/geosite/aws.list",
            ]
        );
        assert!(fallback_urls("https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/ai.list").is_empty());
    }
}
