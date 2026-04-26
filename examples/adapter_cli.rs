//! 统一协议调试 CLI
//!
//! 接受 OpenAI JSON 请求体，支持输出原始 DeepSeek SSE、转换后的 OpenAI SSE 或两者对照。
//!
//! 使用方式:
//!   交互模式: cargo run --example adapter_cli
//!   脚本模式: cargo run --example adapter_cli -- source examples/adapter_cli-script.txt
//!
//! 命令:
//!   chat <json_file>                       - OpenAI 转换后输出
//!   raw <json_file>                        - 原始 DeepSeek SSE（转换前）
//!   compare <json_file>                    - 上下对照两种流
//!   concurrent <n> <json_file>             - 并发请求
//!   models                                 - 列出可用模型
//!   model <id>                             - 查询单个模型
//!   status                                 - 查看账号池状态
//!   source <file>                          - 从文件读取命令执行
//!   quit | exit                            - 退出并清理

use bytes::Bytes;
use ds_free_api::{Config, OpenAIAdapter, StreamResponse};
use futures::{StreamExt, future::join_all};
use std::io::{self, Read, Write};
use std::path::Path;

fn read_line_lossy() -> io::Result<String> {
    let mut buf = Vec::new();
    let mut handle = io::stdin().lock();
    loop {
        let mut byte = [0u8; 1];
        match handle.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                if byte[0] != b'\r' {
                    buf.push(byte[0]);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("info")).init();

    let config = Config::load_with_args(std::env::args())?;
    println!("[初始化中...]");
    let adapter = OpenAIAdapter::new(&config).await?;
    println!(
        "[就绪] 命令: chat | raw | compare | concurrent | models | model | status | source | quit"
    );

    let mut stdout = io::stdout();

    loop {
        print!("> ");
        stdout.flush()?;

        let line = read_line_lossy()?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if handle_line(line, &adapter).await? {
            break;
        }
    }

    println!("[清理中...]");
    adapter.shutdown().await;
    println!("[已关闭]");

    Ok(())
}

fn parse_args<'a>(parts: &'a [&'a str]) -> (Vec<&'a str>, bool) {
    let raw = parts.iter().any(|p| *p == "--raw" || *p == "-r");
    let positional: Vec<_> = parts
        .iter()
        .filter(|p| **p != "--raw" && **p != "-r")
        .copied()
        .collect();
    (positional, raw)
}

async fn handle_line(line: &str, adapter: &OpenAIAdapter) -> anyhow::Result<bool> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(false);
    }

    let cmd = parts[0];
    match cmd {
        "status" => {
            println!("[账号状态]");
            for (i, s) in adapter.account_statuses().iter().enumerate() {
                let email = if s.email.is_empty() { "-" } else { &s.email };
                let mobile = if s.mobile.is_empty() { "-" } else { &s.mobile };
                println!("  [{}] {} / {}", i + 1, email, mobile);
            }
        }

        "chat" if parts.len() >= 2 => {
            let file = parts[1];
            let body = load_json(file)?;
            println!(">>> chat: {}", file);
            let result = adapter.chat_completions_stream(&body).await?;
            println!("[account: {}]", result.account_id);
            let mut stream = result.data;
            print_stream(&mut stream, false).await;
        }

        "raw" if parts.len() >= 2 => {
            let file = parts[1];
            let body = load_json(file)?;
            println!(">>> raw: {}", file);
            let result = adapter.raw_chat_stream(&body).await?;
            println!("[account: {}]", result.account_id);
            let mut stream = result.data;
            print_stream(&mut stream, true).await;
        }

        "compare" if parts.len() >= 2 => {
            let file = parts[1];
            let body = load_json(file)?;
            println!(">>> compare: {}", file);

            // 原始流
            println!("\n═══ RAW DEEPSEEK SSE ═══════════════════════════════════════");
            let raw_result = adapter.raw_chat_stream(&body).await?;
            println!("[account: {}]", raw_result.account_id);
            consume_stream(raw_result.data, |bytes| {
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    println!("  {}", line);
                }
            })
            .await;

            // 转换后流
            println!("\n═══ CONVERTED OPENAI SSE ═══════════════════════════════════");
            let converted_result = adapter.chat_completions_stream(&body).await?;
            println!("[account: {}]", converted_result.account_id);
            consume_stream(converted_result.data, |bytes| {
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    println!("  {}", line);
                }
            })
            .await;

            println!("\n═══ END ════════════════════════════════════════════════════");
        }

        "concurrent" if parts.len() >= 3 => {
            let (positional, raw) = parse_args(&parts);
            let count: usize = match positional[1].parse() {
                Ok(n) if n > 0 => n,
                _ => {
                    eprintln!("[错误] 并发数必须是正整数");
                    return Ok(false);
                }
            };
            let file = positional[2];
            let body = load_json(file)?;
            println!(">>> concurrent: count={}, file={}", count, file);
            run_concurrent(adapter, count, body, raw).await;
        }

        "models" => {
            let json = adapter.list_models();
            println!("{}", String::from_utf8_lossy(&json));
        }

        "model" if parts.len() == 2 => {
            if let Some(json) = adapter.get_model(parts[1]) {
                println!("{}", String::from_utf8_lossy(&json));
            } else {
                println!("null");
            }
        }

        "source" if parts.len() == 2 => {
            let file = parts[1];
            if !Path::new(file).exists() {
                eprintln!("[错误] 文件不存在: {}", file);
                return Ok(false);
            }
            println!("[执行脚本: {}]", file);
            let content = std::fs::read_to_string(file)?;
            for script_line in content.lines() {
                let script_line = script_line.trim();
                if script_line.is_empty() || script_line.starts_with('#') {
                    continue;
                }
                println!(">>> {}", script_line);
                if Box::pin(handle_line(script_line, adapter)).await? {
                    return Ok(true);
                }
            }
            println!("[脚本执行完毕]");
        }

        "quit" | "exit" => {
            println!("[退出]");
            return Ok(true);
        }

        _ => {
            println!(
                "[未知命令: {}] 可用: chat | raw | compare | concurrent | models | model | status | source | quit",
                cmd
            );
        }
    }

    Ok(false)
}

fn load_json(file: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("文件不存在: {}", file);
    }
    Ok(std::fs::read(path)?)
}

/// 消费流并给每个 chunk 应用处理函数
async fn consume_stream<F>(stream: StreamResponse, mut f: F)
where
    F: FnMut(Bytes),
{
    let mut stream = stream;
    while let Some(res) = stream.next().await {
        match res {
            Ok(bytes) => f(bytes),
            Err(e) => {
                eprintln!("\n[流错误] {}", e);
                break;
            }
        }
    }
}

/// 打印流式响应
async fn print_stream(stream: &mut StreamResponse, raw: bool) {
    let mut stdout = io::stdout();
    while let Some(res) = stream.next().await {
        match res {
            Ok(bytes) => {
                if raw {
                    print!("{}", String::from_utf8_lossy(&bytes));
                    stdout.flush().unwrap();
                } else {
                    print_stream_chunk(&bytes);
                }
            }
            Err(e) => {
                eprintln!("\n[流错误] {}", e);
                break;
            }
        }
    }
    if !raw {
        println!();
    }
}

/// 打印单个转换后 chunk 的摘要
fn print_stream_chunk(bytes: &Bytes) {
    let text = String::from_utf8_lossy(bytes);
    let json_str = text
        .strip_prefix("data: ")
        .and_then(|s| s.strip_suffix("\n\n"))
        .unwrap_or(&text);

    let v: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(val) => val,
        Err(_) => {
            print!("{}", text);
            return;
        }
    };

    let choice = v.get("choices").and_then(|c| c.get(0));
    let delta = choice.and_then(|c| c.get("delta"));
    let content = delta
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str());
    let reasoning = delta
        .and_then(|d| d.get("reasoning_content"))
        .and_then(|c| c.as_str());
    let tool_calls = delta.and_then(|d| d.get("tool_calls"));
    let finish = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|f| f.as_str());
    let usage = v.get("usage");

    if choice.is_none() || usage.is_some() {
        if let Some(u) = usage {
            println!("[usage] {}", u);
            return;
        }
    }

    let mut parts = Vec::new();
    if let Some(c) = content {
        parts.push(format!("content={:?}", c));
    }
    if let Some(r) = reasoning {
        parts.push(format!("reasoning={:?}", r));
    }
    if let Some(t) = tool_calls {
        parts.push(format!(
            "tool_calls={}",
            serde_json::to_string(t).unwrap_or_default()
        ));
    }
    if let Some(f) = finish {
        parts.push(format!("finish={}", f));
    }

    if !parts.is_empty() {
        println!("[chunk] {}", parts.join(" | "));
    }
}

/// 执行并发请求
async fn run_concurrent(adapter: &OpenAIAdapter, count: usize, body: Vec<u8>, raw: bool) {
    let start = std::time::Instant::now();
    let is_streaming = is_stream(&body);

    let futures: Vec<_> = (0..count)
        .map(|i| {
            let body = body.clone();
            async move {
                let req_start = std::time::Instant::now();
                let result = if is_streaming {
                    match adapter.chat_completions_stream(&body).await {
                        Ok(result) => {
                            let mut stream = result.data;
                            let mut output = String::new();
                            let mut ok = true;
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(bytes) => {
                                        if raw {
                                            output.push_str(&String::from_utf8_lossy(&bytes));
                                        } else {
                                            let text = String::from_utf8_lossy(&bytes);
                                            let json_str = text
                                                .strip_prefix("data: ")
                                                .and_then(|s| s.strip_suffix("\n\n"))
                                                .unwrap_or(&text);
                                            if let Ok(v) =
                                                serde_json::from_str::<serde_json::Value>(json_str)
                                            {
                                                let delta = v
                                                    .get("choices")
                                                    .and_then(|c| c.get(0))
                                                    .and_then(|c| c.get("delta"));
                                                if let Some(c) = delta
                                                    .and_then(|d| d.get("content"))
                                                    .and_then(|c| c.as_str())
                                                {
                                                    output.push_str(c);
                                                }
                                                if let Some(r) = delta
                                                    .and_then(|d| d.get("reasoning_content"))
                                                    .and_then(|c| c.as_str())
                                                {
                                                    if !output.is_empty() {
                                                        output.push(' ');
                                                    }
                                                    output.push_str(r);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("\n[请求{} 流错误] {}", i, e);
                                        ok = false;
                                        break;
                                    }
                                }
                            }
                            (i, ok, output, req_start.elapsed())
                        }
                        Err(e) => {
                            eprintln!("[请求{} 失败] {}", i, e);
                            (i, false, String::new(), req_start.elapsed())
                        }
                    }
                } else {
                    match adapter.chat_completions(&body).await {
                        Ok(result) => {
                            let json = result.data;
                            let output = if raw {
                                String::from_utf8_lossy(&json).to_string()
                            } else {
                                let v: serde_json::Value =
                                    serde_json::from_slice(&json).unwrap_or_default();
                                let mut parts = Vec::new();
                                if let Some(c) = v
                                    .get("choices")
                                    .and_then(|c| c.get(0))
                                    .and_then(|c| c.get("message"))
                                    .and_then(|m| m.get("content"))
                                    .and_then(|c| c.as_str())
                                {
                                    parts.push(c.to_string());
                                }
                                if let Some(r) = v
                                    .get("choices")
                                    .and_then(|c| c.get(0))
                                    .and_then(|c| c.get("message"))
                                    .and_then(|m| m.get("reasoning_content"))
                                    .and_then(|c| c.as_str())
                                {
                                    parts.push(r.to_string());
                                }
                                parts.join(" ")
                            };
                            (i, true, output, req_start.elapsed())
                        }
                        Err(e) => {
                            eprintln!("[请求{} 失败] {}", i, e);
                            (i, false, String::new(), req_start.elapsed())
                        }
                    }
                };
                result
            }
        })
        .collect();

    let results = join_all(futures).await;
    let total_elapsed = start.elapsed();

    println!("\n[并发结果]");
    let success_count = results.iter().filter(|(_, ok, _, _)| *ok).count();
    for (i, ok, output, elapsed) in results {
        let preview: String = output.chars().take(80).collect();
        let status = if ok { "成功" } else { "失败" };
        println!(
            "  [请求{:2}] {} | {:>12?} | {}",
            i,
            status,
            elapsed,
            if preview.is_empty() {
                "(无输出)".to_string()
            } else {
                format!("{}...", preview.replace('\n', " "))
            }
        );
    }
    println!(
        "  总计: {}/{} 成功 | 总耗时 {:?}",
        success_count, count, total_elapsed
    );
}

/// 判断请求体是否要求流式
fn is_stream(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(|s| s.as_bool()))
        .unwrap_or(false)
}
