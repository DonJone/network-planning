use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// 拉取远程文本，支持重试；非 UTF-8 字节直接丢弃
/// 显式声明 identity 编码：jsDelivr 对 `Accept-Encoding: gzip` 的部分路径会返回 403
pub fn fetch_text(url: &str, user_agent: &str, retries: u32, timeout: Duration, backoff: Duration) -> Result<String, String> {
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

fn decode_utf8_ignore(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
    }
    text
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
