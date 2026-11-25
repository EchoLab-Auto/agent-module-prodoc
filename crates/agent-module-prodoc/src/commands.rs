//! ProDoc 命令域。
//!
//! | 命令 | 用途 | 只读 |
//! |---|---|---|
//! | `RequestProDocBundle` | 打开一个文档根，推回全量文件快照 | 是 |
//! | `BrowseProDocDirs` | 目录选择器的逐层浏览（只列子目录） | 是 |
//! | `ProDocRoots` | 可打开的根列表（配置 + 各人格工作区目录） | 是 |
//! | `ReadProDocFile` | 读单文件原文（冲突对照 / 合并基准） | 是 |
//! | `SaveProDocFile` | 保存单文件（带 `base` 冲突检测） | **否** |
//! | `DeleteProDocFile` | 删除单文件（带 `base` 冲突检测） | **否** |
//!
//! 命令本身**只回执成功/失败**，文档内容走模块事件（见 `lib.rs` 的事件协议）：
//! 一份快照可能几百 KB，塞进命令回执会和面板的轮询/重放逻辑缠在一起，而事件
//! 通道本来就为"服务器主动推"设计的。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::bundle::{self, ScanError};
use crate::check;
use crate::write::{self, WriteOutcome};
use crate::{events, Caps, DirEntry, ModuleState, RootEntry, WriteAck};

/// 检查命令名（宿主侧 `prodoc_check` 工具按它分发——写成常量而不是两处
/// 字面量：工具与命令的名字一旦漂移，表现是"工具存在但永远说模块未启用"）。
pub const CHECK_COMMAND: &str = "ProDocCheck";

/// 本模块处理的命令（`Contribution::Command` 的 `name`）。
pub const COMMANDS: [&str; 7] = [
    "RequestProDocBundle",
    "BrowseProDocDirs",
    "ProDocRoots",
    "ReadProDocFile",
    "SaveProDocFile",
    "DeleteProDocFile",
    "ProDocCheck",
];

/// 命令入口（返回 `None` = 不认领，不是错误）。
pub async fn handle(
    state: &ModuleState,
    name: &str,
    payload: Value,
) -> Option<Result<Value, String>> {
    if !COMMANDS.contains(&name) {
        return None;
    }
    let team_id = payload
        .get("team_id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string);

    let outcome = match name {
        "RequestProDocBundle" => request_bundle(state, team_id, &payload).await,
        "BrowseProDocDirs" => browse(state, &payload),
        "ProDocRoots" => roots(state),
        "ReadProDocFile" => read_file(state, team_id, &payload),
        "SaveProDocFile" => save_file(state, team_id, &payload).await,
        "DeleteProDocFile" => delete_file(state, team_id, &payload).await,
        "ProDocCheck" => pro_check(state, team_id, &payload).await,
        other => return Some(Err(format!("命令 {other} 被声明但处理器没有认领它"))),
    };
    Some(outcome)
}

/// 打开文档根并推回快照（`RequestProDocBundle`）。
async fn request_bundle(
    state: &ModuleState,
    team_id: Option<String>,
    payload: &Value,
) -> Result<Value, String> {
    let config = state.config();
    let raw = payload
        .get("root")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .ok_or_else(|| "RequestProDocBundle 缺少 root".to_string())?;
    let root = PathBuf::from(raw);

    // 沙箱：只允许配置里显式给出的根，或它们的子目录。
    // 不做"任意绝对路径"——那等于给面板开了一个读全盘 `.md` 的口子。
    if let Err(message) = check_allowed(&config.roots, &root) {
        state.emit_error(team_id.as_deref(), raw, &message);
        return Err(message);
    }

    // 扫描是阻塞 IO（几千个文件几百毫秒），必须在阻塞线程池里跑：
    // 这条命令走的是宿主单条命令通道，阻塞它会连累其他模块的命令。
    let scan_root = root.clone();
    let scan_config = config.clone();
    let scanned = tokio::task::spawn_blocking(move || bundle::scan(&scan_root, &scan_config))
        .await
        .map_err(|error| format!("文档扫描失败: {error}"))?;

    match scanned {
        Ok(mut bundle) => {
            // 标记校验（`require_marker`，2026-11）：只有声明了自己是文档群的
            // 目录才允许打开。没有这一条，自动发现就只是"建议"——用户仍可用
            // 任意路径打开一个塞满 README 的仓库根，画出一张没有结构的图。
            if config.require_marker && !bundle.prodoc {
                let message = marker_error(raw);
                state.emit_error(team_id.as_deref(), raw, &message);
                return Err(message);
            }
            bundle.team_id = team_id.clone();
            let documents = bundle.stats.documents;
            let bytes = bundle.stats.bytes;
            state.emit_bundle(&bundle);
            Ok(json!({
                "root": bundle.root,
                "documents": documents,
                "bytes": bytes,
                "truncated": bundle.stats.truncated,
            }))
        }
        Err(error) => {
            let message = match &error {
                ScanError::RootUnavailable(text) => text.clone(),
                ScanError::Empty => format!("{}里没有任何 .md 文件", raw),
            };
            state.emit_error(team_id.as_deref(), raw, &message);
            Err(message)
        }
    }
}

/// 文档图检查（`ProDocCheck`）。
///
/// 扫描 + 检查一起在阻塞线程里跑：两者都是纯 CPU + 文件读，一个几千篇的文档群
/// 能把命令通道占住几百毫秒。报告走事件（`prodoc.report`），回执只给统计。
async fn pro_check(
    state: &ModuleState,
    team_id: Option<String>,
    payload: &Value,
) -> Result<Value, String> {
    let config = state.config();
    let raw = required(payload, "root", "ProDocCheck")?;
    let root = PathBuf::from(raw);
    if let Err(message) = check_allowed(&config.roots, &root) {
        state.emit_error(team_id.as_deref(), raw, &message);
        return Err(message);
    }

    let scan_root = root.clone();
    let scan_config = config.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let bundle = bundle::scan(&scan_root, &scan_config)?;
        // 把"是否声明为文档群"带出阻塞块：检查报告与标记校验是两件事，
        // 但都要同一份扫描结果，分两次扫描等于读两遍目录。
        let prodoc = bundle.prodoc;
        Ok::<_, ScanError>((prodoc, check::check(&bundle.files)))
    })
    .await
    .map_err(|error| format!("检查任务失败: {error}"))?;

    let (prodoc, report) = match outcome {
        Ok(value) => value,
        Err(error) => {
            let message = match &error {
                ScanError::RootUnavailable(text) => text.clone(),
                ScanError::Empty => format!("{raw}里没有任何 .md 文件"),
            };
            state.emit_error(team_id.as_deref(), raw, &message);
            return Err(message);
        }
    };
    if config.require_marker && !prodoc {
        let message = marker_error(raw);
        state.emit_error(team_id.as_deref(), raw, &message);
        return Err(message);
    }
    state.emit_report(team_id.as_deref(), raw, &report);
    // 回执带**完整报告**：面板走事件（不看回执），而工具代理拿的是回执——
    // 它要能把逐条问题念给模型听。两条出口共用同一份数据，不额外存状态。
    //
    // 用 `CheckReceipt` 而不是 `json!` 现场拼：宿主侧读的是**同一个类型**，
    // 字段改名会让两侧一起编译失败，而不是让工具静默报告"0 个错误"。
    serde_json::to_value(crate::check::CheckReceipt::from_report(raw, &report))
        .map_err(|error| format!("检查回执序列化失败: {error}"))
}

/// 未声明为文档群时的错误文案。
///
/// 措辞要同时回答两件事：**为什么打不开** 和 **怎么让它能打开**。
/// 只说"不是文档群"用户会去翻文档；把标记字段名写进文案，作者当场就能改。
fn marker_error(raw: &str) -> String {
    format!(
        "{raw} 里没有文档群声明：至少一篇文档的参数区要写 `{}: true`",
        bundle::MARKER_KEY
    )
}

// ── 单文件读写 ───────────────────────────────────────────────────────────

/// 从载荷里取必填字段（三个写命令共用的形状）。
fn required<'a>(payload: &'a Value, field: &str, command: &str) -> Result<&'a str, String> {
    payload
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{command} 缺少 {field}"))
}

/// 三个写命令的共同前置：解析 root / path / request_id，并校验 root 在允许范围内。
fn write_target<'a>(
    state: &ModuleState,
    team_id: Option<&str>,
    payload: &'a Value,
    command: &str,
) -> Result<(std::path::PathBuf, &'a str, Option<&'a str>), String> {
    let config = state.config();
    let root_raw = required(payload, "root", command)?;
    let path = required(payload, "path", command)?;
    let root = PathBuf::from(root_raw);
    if let Err(message) = check_allowed(&config.roots, &root) {
        state.emit_error(team_id, root_raw, &message);
        return Err(message);
    }
    let request_id = payload
        .get("request_id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty());
    Ok((root, path, request_id))
}

/// 读单文件（`ReadProDocFile`）：冲突对照与合并基准都靠它。
fn read_file(
    state: &ModuleState,
    team_id: Option<String>,
    payload: &Value,
) -> Result<Value, String> {
    let (root, path, request_id) =
        write_target(state, team_id.as_deref(), payload, "ReadProDocFile")?;
    let mut ack = write::read(&root, path);
    ack.request_id = request_id.map(str::to_string);
    ack.team_id = team_id;
    let ok = ack.ok;
    state.emit_write(events::FILE, ack);
    Ok(json!({"ok": ok}))
}

/// 保存单文件（`SaveProDocFile`）。
///
/// 判断全在 [`crate::write::save`]，这里只做"把结果翻译成回执"——两者分开是
/// 因为写盘语义要与 CLI 逐条对齐，掺进命令解析会看不清哪条是"CLI 也这么做"、
/// 哪条是"我们额外加的"。
async fn save_file(
    state: &ModuleState,
    team_id: Option<String>,
    payload: &Value,
) -> Result<Value, String> {
    let (root, path, request_id) =
        write_target(state, team_id.as_deref(), payload, "SaveProDocFile")?;
    let content = required(payload, "content", "SaveProDocFile")?.to_string();
    let base = payload.get("base").and_then(|value| value.as_str());
    // 写盘是阻塞 IO：文档目录可能在网络盘上，一次 rename 就能卡住整条命令通道。
    let root_for_task = root.clone();
    let path_for_task = path.to_string();
    let base_for_task = base.map(str::to_string);
    let outcome = tokio::task::spawn_blocking(move || {
        write::save(
            &root_for_task,
            &path_for_task,
            &content,
            base_for_task.as_deref(),
        )
    })
    .await
    .map_err(|error| format!("保存任务失败: {error}"))?;

    let mut ack = WriteAck {
        request_id: request_id.map(str::to_string),
        team_id,
        root: root.to_string_lossy().into_owned(),
        path: path.to_string(),
        ..Default::default()
    };
    match outcome {
        WriteOutcome::Written => ack.ok = true,
        WriteOutcome::Conflict { disk, error } => {
            ack.conflict = true;
            ack.disk = disk;
            ack.error = Some(error);
        }
        WriteOutcome::Failed(message) => ack.error = Some(message),
    }
    let ok = ack.ok;
    state.emit_write(events::SAVED, ack);
    Ok(json!({"ok": ok}))
}

/// 删除单文件（`DeleteProDocFile`）。
async fn delete_file(
    state: &ModuleState,
    team_id: Option<String>,
    payload: &Value,
) -> Result<Value, String> {
    let (root, path, request_id) =
        write_target(state, team_id.as_deref(), payload, "DeleteProDocFile")?;
    let base = payload.get("base").and_then(|value| value.as_str());
    let root_for_task = root.clone();
    let path_for_task = path.to_string();
    let base_for_task = base.map(str::to_string);
    let outcome = tokio::task::spawn_blocking(move || {
        write::delete(&root_for_task, &path_for_task, base_for_task.as_deref())
    })
    .await
    .map_err(|error| format!("删除任务失败: {error}"))?;

    let mut ack = WriteAck {
        request_id: request_id.map(str::to_string),
        team_id,
        root: root.to_string_lossy().into_owned(),
        path: path.to_string(),
        ..Default::default()
    };
    match outcome {
        WriteOutcome::Written => ack.ok = true,
        WriteOutcome::Conflict { disk, error } => {
            ack.conflict = true;
            ack.disk = disk;
            ack.error = Some(error);
        }
        WriteOutcome::Failed(message) => ack.error = Some(message),
    }
    let ok = ack.ok;
    state.emit_write(events::DELETED, ack);
    Ok(json!({"ok": ok}))
}

/// 目录选择器：列 `path` 下的子目录（`BrowseProDocDirs`）。
///
/// 刻意**不**复用工作区模块的浏览根：那套以"全部人格工作区目录 ∪ HOME"为根，
/// 面向的是"给会话选工作目录"；文档根是"我想看这棵文档" —— 边界由配置的
/// 发现范围 `roots` 决定，语义不同。
///
/// 空路径返回的是**发现的文档群**（`discover`），不是配置原样：选择器只在
/// "没有自动发现结果、想手点一个"时才作为兜底入口，列出配置路径没有意义。
fn browse(state: &ModuleState, payload: &Value) -> Result<Value, String> {
    let config = state.config();
    let requested = payload
        .get("path")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|path| !path.is_empty());
    // 空路径 = 根列表（发现的文档群）。
    let Some(requested) = requested else {
        let found = discover(&config);
        state.emit_dirs(None, &found, &[]);
        return Ok(json!({"roots": found.len(), "entries": 0}));
    };

    let path = PathBuf::from(requested);
    check_allowed(&config.roots, &path)?;
    let entries = list_child_dirs(&path)?;
    state.emit_dirs(Some(requested), &[], &entries);
    Ok(json!({"path": requested, "entries": entries.len()}))
}

/// 可打开的根列表（`ProDocRoots`）——即**发现的文档群**。
///
/// 语义（2026-11 起）：不是"配置里写了哪些路径"，而是"在配置的发现范围里，
/// 哪些目录真的声明了自己是文档群"。判据是参数区里的 `prodoc: true`
/// （见 [`crate::bundle::MARKER_KEY`]）。
///
/// 为什么不直接列配置：`roots` 是**发现范围**（通常是几个人格工作区目录，
/// 每个下面有几十个子目录），把它原样丢给面板的结果是用户要在一堆仓库里
/// 自己认出哪个是文档群。原来那版还会在配置为空时回退 HOME，等于把整个家
/// 目录当候选。
fn roots(state: &ModuleState) -> Result<Value, String> {
    let config = state.config();
    let entries = discover(&config);
    state.emit_roots(&entries, &Caps::of(&config));
    Ok(json!({
        "roots": entries.len(),
        "configured": !config.roots.is_empty(),
        "require_marker": config.require_marker,
    }))
}

/// 在发现范围里递归找文档群。
///
/// 规则（每一条都有它挡掉的具体误判）：
///
/// - **命中即止**：一个目录自己带标记就不再往下走。文档群是嵌套的
///   （`<repo>/document/{core,modules,...}`），不剪枝会把同一个群报成十几个根。
/// - **realpath 去重**：符号链接成环或两个 root 互相包含时不重复报。
/// - **深度上限**：`config.discover_depth`。文档群实际都在 1–2 层内，深挖只会
///   在大仓库上白跑。
/// - **跳过产物目录与隐藏目录**：与扫描器同一份 [`crate::bundle::SKIP_DIRS`]，
///   否则 `node_modules` 会让发现慢到不可用。
fn discover(config: &crate::Config) -> Vec<RootEntry> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for root in allowed_roots(config) {
        walk_for_marker(&root, 0, config.discover_depth, &mut found, &mut seen);
    }
    found.sort();
    found.dedup();
    let labels = disambiguated_labels(&found);
    found
        .into_iter()
        .zip(labels)
        .map(|(root, label)| RootEntry {
            label,
            hint: home_relative(&root),
            path: root.to_string_lossy().into_owned(),
            prodoc: true,
        })
        .collect()
}

/// [`discover`] 的递归体。
fn walk_for_marker(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    found: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    let Ok(real) = std::fs::canonicalize(dir) else {
        return;
    };
    if !real.is_dir() || !seen.insert(real.clone()) {
        return;
    }
    if crate::bundle::dir_has_marker(&real) {
        found.push(real);
        return; // 命中即止：子目录归这个群
    }
    if depth >= max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&real) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            !name.starts_with('.') && !crate::bundle::SKIP_DIRS.contains(&name.as_str()) && name != "target"
        })
        .map(|entry| entry.path())
        .collect();
    // 排序：目录顺序随文件系统变化，不排会让"发现结果的顺序"每次不同，
    // 面板列表跟着跳动。
    children.sort();
    for child in children {
        walk_for_marker(&child, depth + 1, max_depth, found, seen);
    }
}

/// 给每个根算一个**彼此不重名**的显示名。
///
/// 从最后一段开始，只要有重名就把所有候选一起往上加一层，直到全不相同或
/// 已经吃到根（`/`）。为什么是"一起加"而不是"只把撞的那个加长"：
/// 后者会让 `document` 与 `DocRenderer/document` 并排出现，两条信息长度差
/// 一倍，扫读时反而更乱。
fn disambiguated_labels(roots: &[PathBuf]) -> Vec<String> {
    if roots.is_empty() {
        return Vec::new();
    }
    let max_depth = roots
        .iter()
        .map(|root| {
            root.components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .count()
        })
        .max()
        .unwrap_or(1)
        .max(1);

    let mut depth = 1;
    loop {
        let labels: Vec<String> = roots
            .iter()
            .map(|root| tail_segments(root, depth))
            .collect();
        let unique = labels
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == labels.len();
        // 到最深层还撞（两个根字面相同）就没得再区分了，接受重复。
        if unique || depth >= max_depth {
            return labels;
        }
        depth += 1;
    }
}

/// 路径末 `depth` 段用 `/` 连接（`depth` 超过段数时给全路径）。
fn tail_segments(path: &Path, depth: usize) -> String {
    let segments: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let take = depth.min(segments.len()).max(1);
    segments[segments.len() - take..].join("/")
}

/// 家目录缩写成 `~`（只在真落在 HOME 下时改，避免把别的绝对路径写歪）。
fn home_relative(path: &Path) -> String {
    let full = path.to_string_lossy().into_owned();
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return full;
    };
    let home_text = home.to_string_lossy().into_owned();
    if home_text.is_empty() || home_text == "/" {
        return full;
    }
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => full,
    }
}

/// 生效的根列表：配置优先，空则回退 HOME。
fn allowed_roots(config: &crate::Config) -> Vec<PathBuf> {
    let configured: Vec<PathBuf> = config
        .roots
        .iter()
        .map(|root| root.trim())
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .filter(|root| root.is_dir())
        .collect();
    if !configured.is_empty() {
        return configured;
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_dir())
        .into_iter()
        .collect()
}

/// `path` 是否落在某个允许根内（含根本身）。
///
/// 两边都先 `canonicalize`：符号链接的比较必须发生在真实路径上，否则
/// `/home/me/link-to-docs` 与 `/srv/docs` 会被判成两个不相干的位置。
fn check_allowed(configured: &[String], path: &Path) -> Result<(), String> {
    let config = crate::Config {
        roots: configured.to_vec(),
        ..Default::default()
    };
    let roots = allowed_roots(&config);
    if roots.is_empty() {
        return Err("没有可用的文档根（配置里没有 roots，且 HOME 不可用）".into());
    }
    let target = std::fs::canonicalize(path)
        .map_err(|error| format!("路径不可访问：{}（{error}）", path.display()))?;
    for root in &roots {
        let Ok(root_real) = std::fs::canonicalize(root) else {
            continue;
        };
        if target == root_real || target.starts_with(&root_real) {
            return Ok(());
        }
    }
    Err(format!(
        "文档根必须在允许范围内：{}",
        roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("、")
    ))
}

/// 列子目录（不列文件：选择器只关心"能不能进这一层"）。
fn list_child_dirs(path: &Path) -> Result<Vec<DirEntry>, String> {
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("目录不可读：{}（{error}）", path.display()))?;
    let mut dirs: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        if crate::bundle::SKIP_DIRS.contains(&name.as_str()) || name == "target" {
            continue;
        }
        dirs.push((name, entry.path().to_string_lossy().into_owned()));
    }
    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(dirs
        .into_iter()
        .map(|(name, full)| DirEntry {
            name,
            path: full,
            is_dir: true,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "echo-prodoc-cmd-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/guide")).expect("mkdir");
        // 夹具带 `prodoc: true`：2026-11 起「没标记 = 不是文档群」是打开
        // 文档群的前置条件（`require_marker` 默认开），夹具不声明的话下面
        // 一半用例都会撞在标记校验上，测的就不是它们本来要测的东西了。
        std::fs::write(
            dir.join("docs/project.md"),
            "---\nprodoc: true\nid: p\n---\n# P\n",
        )
        .expect("write");
        std::fs::write(dir.join("docs/guide/a.md"), "# A\n").expect("write");
        dir
    }

    /// 空的临时目录（不带任何预制文档）。
    ///
    /// 发现类用例要自己布置整棵树：`temp_dir` 里那份带标记的 `docs/` 混进来
    /// 会让断言变成"依赖别的用例的种子"。
    fn bare_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "echo-prodoc-scan-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn state_with(root: &Path) -> ModuleState {
        ModuleState::new(Config {
            roots: vec![root.to_string_lossy().into_owned()],
            ..Config::default()
        })
    }

    /// 认领语义：无关命令返回 `None`。
    #[tokio::test]
    async fn unknown_command_is_not_claimed() {
        let state = ModuleState::default();
        assert!(handle(&state, "RequestState", json!({})).await.is_none());
    }

    /// 打开文档根 → 推一份快照事件，回执带统计。
    #[tokio::test]
    async fn request_bundle_emits_snapshot() {
        let dir = temp_dir("bundle");
        let state = state_with(&dir.join("docs"));
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let seen = seen.clone();
            state.set_event_sink(std::sync::Arc::new(move |event| {
                seen.lock()
                    .unwrap()
                    .push((event.name.clone(), event.payload.clone()));
            }));
        }

        let receipt = handle(
            &state,
            "RequestProDocBundle",
            json!({"team_id": "t", "root": dir.join("docs").to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect("ok");
        assert_eq!(receipt["documents"], 2);

        let events = seen.lock().unwrap().clone();
        let (name, payload) = events
            .iter()
            .find(|(name, _)| name == crate::events::BUNDLE)
            .expect("bundle event");
        assert_eq!(name, crate::events::BUNDLE);
        assert_eq!(payload["team_id"], "t");
        assert!(payload["files"]["guide/a.md"].is_string());
        assert_eq!(payload["stats"]["documents"], 2);
    }

    /// 根不在允许范围 → 明确报错 + 错误事件（面板要能显示"为什么打不开"）。
    #[tokio::test]
    async fn root_outside_allowed_is_rejected() {
        let dir = temp_dir("outside");
        let state = state_with(&dir.join("docs"));
        let outside = std::env::temp_dir().join("echo-prodoc-elsewhere");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::write(outside.join("x.md"), "# x\n").expect("write");

        let outcome = handle(
            &state,
            "RequestProDocBundle",
            json!({"team_id": "t", "root": outside.to_string_lossy()}),
        )
        .await
        .expect("claimed");
        assert!(outcome.unwrap_err().contains("允许范围"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// 目录选择器：空路径给根列表，给路径给子目录。
    ///
    /// 断言走**事件**而不是命令回执：宿主把 `Ok(_)` 的返回值直接丢弃
    /// （`apply_command` 只认 `Err` 发错误），面板能看见的只有事件。
    #[tokio::test]
    async fn browse_emits_roots_then_children() {
        let dir = temp_dir("browse");
        let state = state_with(&dir.join("docs"));
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let seen = seen.clone();
            state.set_event_sink(std::sync::Arc::new(move |event| {
                seen.lock()
                    .unwrap()
                    .push((event.name.clone(), event.payload.clone()));
            }));
        }

        handle(&state, "BrowseProDocDirs", json!({}))
            .await
            .expect("claimed")
            .expect("ok");
        let roots_event = seen
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| name == crate::events::DIRS)
            .map(|(_, payload)| payload.clone())
            .expect("应有 prodoc.dirs 事件");
        assert_eq!(roots_event["roots"].as_array().unwrap().len(), 1);

        handle(
            &state,
            "BrowseProDocDirs",
            json!({"path": dir.join("docs").to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect("ok");
        let events = seen.lock().unwrap().clone();
        let children_event = events
            .iter()
            .rev()
            .find(|(name, _)| name == crate::events::DIRS)
            .map(|(_, payload)| payload.clone())
            .expect("应有第二条 prodoc.dirs");
        let names: Vec<String> = children_event["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["guide".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写命令的事件收集器（与快照测试共用形状）。
    fn collect_writes(
        state: &ModuleState,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> {
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        state.set_event_sink(std::sync::Arc::new(move |event| {
            sink.lock()
                .unwrap()
                .push((event.name.clone(), event.payload.clone()));
        }));
        seen
    }

    fn writes_of(
        seen: &std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
        name: &str,
    ) -> Vec<Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    /// 保存：带 base 且磁盘未变 → 写盘并回 `saved.ok`（request_id 原样带回）。
    #[tokio::test]
    async fn save_writes_and_echoes_request_id() {
        let dir = temp_dir("save");
        let root = dir.join("docs");
        std::fs::write(root.join("a.md"), "v1").expect("seed");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        handle(
            &state,
            "SaveProDocFile",
            json!({
                "team_id": "t", "root": root.to_string_lossy(), "path": "a.md",
                "content": "v2", "base": "v1", "request_id": "req-1",
            }),
        )
        .await
        .expect("claimed")
        .expect("ok");

        assert_eq!(std::fs::read_to_string(root.join("a.md")).unwrap(), "v2");
        let acks = writes_of(&seen, crate::events::SAVED);
        assert_eq!(acks.len(), 1);
        assert_eq!(acks[0]["request_id"], "req-1");
        assert_eq!(acks[0]["ok"], true);
        assert_eq!(acks[0]["conflict"], false);
    }

    /// 保存冲突：**不写盘**，回执带磁盘当前内容（前端靠它跑 diff3）。
    #[tokio::test]
    async fn save_conflict_returns_disk_content() {
        let dir = temp_dir("save-conflict");
        let root = dir.join("docs");
        std::fs::write(root.join("a.md"), "disk-edit").expect("seed");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        handle(
            &state,
            "SaveProDocFile",
            json!({
                "team_id": "t", "root": root.to_string_lossy(), "path": "a.md",
                "content": "mine", "base": "stale-base",
            }),
        )
        .await
        .expect("claimed")
        .expect("ok");

        assert_eq!(
            std::fs::read_to_string(root.join("a.md")).unwrap(),
            "disk-edit",
            "冲突时不该写盘"
        );
        let acks = writes_of(&seen, crate::events::SAVED);
        assert_eq!(acks[0]["conflict"], true);
        assert_eq!(acks[0]["disk"], "disk-edit");
    }

    /// 读取：原文逐字返回（末尾无换行也要一致，否则 base 比对永远不等）。
    #[tokio::test]
    async fn read_returns_raw_content() {
        let dir = temp_dir("read");
        let root = dir.join("docs");
        std::fs::write(root.join("a.md"), "---\nid: a\n---\n结尾没有换行").expect("seed");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        handle(
            &state,
            "ReadProDocFile",
            json!({"root": root.to_string_lossy(), "path": "a.md", "request_id": "r1"}),
        )
        .await
        .expect("claimed")
        .expect("ok");

        let acks = writes_of(&seen, crate::events::FILE);
        assert_eq!(acks[0]["ok"], true);
        assert_eq!(acks[0]["content"], "---\nid: a\n---\n结尾没有换行");
    }

    /// 删除：磁盘未变则删掉；变了则冲突且保留文件。
    #[tokio::test]
    async fn delete_respects_base() {
        let dir = temp_dir("delete");
        let root = dir.join("docs");
        std::fs::write(root.join("a.md"), "changed").expect("seed");
        std::fs::write(root.join("b.md"), "as-seen").expect("seed");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        handle(
            &state,
            "DeleteProDocFile",
            json!({"root": root.to_string_lossy(), "path": "a.md", "base": "stale"}),
        )
        .await
        .expect("claimed")
        .expect("ok");
        assert!(root.join("a.md").exists(), "冲突时不该删");

        handle(
            &state,
            "DeleteProDocFile",
            json!({"root": root.to_string_lossy(), "path": "b.md", "base": "as-seen"}),
        )
        .await
        .expect("claimed")
        .expect("ok");
        assert!(!root.join("b.md").exists(), "base 匹配应删掉");

        let acks = writes_of(&seen, crate::events::DELETED);
        assert_eq!(acks[0]["conflict"], true);
        assert_eq!(acks[1]["ok"], true);
    }

    /// 写越权：root 不在白名单 → 错误事件，不碰文件。
    #[tokio::test]
    async fn save_outside_allowed_root_is_rejected() {
        let dir = temp_dir("save-escape");
        let root = dir.join("docs");
        let state = state_with(&root);
        let seen = collect_writes(&state);
        let outside = std::env::temp_dir().join("echo-prodoc-elsewhere-write");
        std::fs::create_dir_all(&outside).expect("mkdir");

        let outcome = handle(
            &state,
            "SaveProDocFile",
            json!({
                "root": outside.to_string_lossy(), "path": "x.md", "content": "boom",
            }),
        )
        .await
        .expect("claimed");
        assert!(outcome.unwrap_err().contains("允许范围"));
        assert!(!outside.join("x.md").exists());
        assert_eq!(
            writes_of(&seen, crate::events::SAVED).len(),
            0,
            "不该发成功回执"
        );
        assert_eq!(writes_of(&seen, crate::events::ERROR).len(), 1);
        let _ = std::fs::remove_dir_all(&outside);
    }

    // ── 文档群标记与自动发现（2026-11）──────────────────────────────────

    /// 未声明为文档群的目录打不开（`require_marker` 默认开）。
    ///
    /// 这条是「禁止」的落地点：没有它，自动发现只是"建议"——用户仍可用任意
    /// 路径打开一个塞满 README 的仓库根，画出一张没有结构的图。
    #[tokio::test]
    async fn unmarked_root_is_rejected() {
        let dir = temp_dir("no-marker");
        // 摘掉标记：夹具默认带 `prodoc: true`
        std::fs::write(dir.join("docs/project.md"), "---\nid: p\n---\n# P\n").expect("write");
        let root = dir.join("docs");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        let error = handle(
            &state,
            "RequestProDocBundle",
            json!({"root": root.to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect_err("未声明为文档群的目录不该打开");
        assert!(error.contains("prodoc: true"), "{error}");

        // 面板靠这个事件显示错误条，而不是空画布
        assert_eq!(writes_of(&seen, crate::events::ERROR).len(), 1);
        assert_eq!(writes_of(&seen, crate::events::BUNDLE).len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ProDocCheck` 同样受标记约束（否则它就是一个"读任意 .md"的旁路）。
    #[tokio::test]
    async fn unmarked_root_check_is_rejected() {
        let dir = temp_dir("no-marker-check");
        std::fs::write(dir.join("docs/project.md"), "---\nid: p\n---\n# P\n").expect("write");
        let root = dir.join("docs");
        let state = state_with(&root);
        let error = handle(
            &state,
            "ProDocCheck",
            json!({"root": root.to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect_err("未声明为文档群的目录不该被检查");
        assert!(error.contains("prodoc: true"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `require_marker = false` 是迁移期的逃生口：恢复旧行为。
    #[tokio::test]
    async fn marker_can_be_disabled() {
        let dir = temp_dir("marker-off");
        std::fs::write(dir.join("docs/project.md"), "---\nid: p\n---\n# P\n").expect("write");
        let root = dir.join("docs");
        let state = ModuleState::new(Config {
            roots: vec![root.to_string_lossy().into_owned()],
            require_marker: false,
            ..Config::default()
        });
        handle(
            &state,
            "RequestProDocBundle",
            json!({"root": root.to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect("关掉 require_marker 后应能打开");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 发现：命中标记即止（父目录吸收子目录），跳过产物目录与隐藏目录。
    #[tokio::test]
    async fn discover_finds_shallowest_marked_dirs() {
        let dir = bare_temp_dir("discover");
        // 一个文档群：document/ 自己带标记，子目录也带 → 只报 document
        std::fs::create_dir_all(dir.join("work/repo/document/core")).expect("mkdir");
        std::fs::write(
            dir.join("work/repo/document/index.md"),
            "---\nprodoc: true\n---\n# 目录\n",
        )
        .expect("write");
        std::fs::write(
            dir.join("work/repo/document/core/a.md"),
            "---\nprodoc: true\n---\n# A\n",
        )
        .expect("write");
        // 普通仓库（无标记）：不该出现在结果里
        std::fs::write(dir.join("work/repo/README.md"), "# 不是文档群\n").expect("write");
        // 产物目录里的标记不算（发现时跳过）
        std::fs::create_dir_all(dir.join("work/repo/node_modules/pkg")).expect("mkdir");
        std::fs::write(
            dir.join("work/repo/node_modules/pkg/x.md"),
            "---\nprodoc: true\n---\n# X\n",
        )
        .expect("write");

        let state = ModuleState::new(Config {
            roots: vec![dir.to_string_lossy().into_owned()],
            ..Config::default()
        });
        let found = discover(&state.config());
        let paths: Vec<&str> = found.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths.len(), 1, "只报最浅的文档群，got {paths:?}");
        assert!(paths[0].ends_with("work/repo/document"), "{paths:?}");
        assert!(found[0].prodoc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 深度上限：标记埋得太深时不报（否则发现会在大仓库上白跑）。
    #[tokio::test]
    async fn discover_respects_depth_limit() {
        let dir = bare_temp_dir("discover-depth");
        std::fs::create_dir_all(dir.join("a/b/c/deep")).expect("mkdir");
        std::fs::write(
            dir.join("a/b/c/deep/x.md"),
            "---\nprodoc: true\n---\n# X\n",
        )
        .expect("write");
        let shallow = ModuleState::new(Config {
            roots: vec![dir.to_string_lossy().into_owned()],
            discover_depth: 1,
            ..Config::default()
        });
        assert!(discover(&shallow.config()).is_empty());
        let deep = ModuleState::new(Config {
            roots: vec![dir.to_string_lossy().into_owned()],
            discover_depth: 5,
            ..Config::default()
        });
        assert_eq!(discover(&deep.config()).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 检查命令：报告走 `prodoc.report` 事件（含 stats 与逐条 issues）。
    #[tokio::test]
    async fn check_command_emits_report() {
        let dir = temp_dir("check");
        // 独立的根：`temp_dir` 那份是按"已有两篇正常文档"预置的，混进来会把
        // 断言变成"依赖别的用例的种子"。
        let root = dir.join("only");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(
            root.join("a.md"),
            "---\nprodoc: true\nid: a\nlink: [\"ghost\"]\n---\n# A\n",
        )
        .expect("seed");
        let state = state_with(&root);
        let seen = collect_writes(&state);

        let receipt = handle(
            &state,
            "ProDocCheck",
            json!({"team_id": "t", "root": root.to_string_lossy()}),
        )
        .await
        .expect("claimed")
        .expect("ok");

        assert_eq!(receipt["documents"], 1);
        assert_eq!(receipt["errors"], 1);
        assert_eq!(receipt["ok"], false);

        let reports = writes_of(&seen, crate::events::REPORT);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["stats"]["documents"], 1);
        assert_eq!(reports[0]["issues"][0]["rule"], "broken-link");
        assert_eq!(reports[0]["ok"], false);
    }

    /// 检查越权：root 不在白名单 → 错误事件，不发报告。
    #[tokio::test]
    async fn check_outside_allowed_root_is_rejected() {
        let dir = temp_dir("check-escape");
        let root = dir.join("docs");
        let state = state_with(&root);
        let seen = collect_writes(&state);
        let outside = std::env::temp_dir().join("echo-prodoc-elsewhere-check");
        std::fs::create_dir_all(&outside).expect("mkdir");

        let outcome = handle(
            &state,
            "ProDocCheck",
            json!({"root": outside.to_string_lossy()}),
        )
        .await
        .expect("claimed");
        assert!(outcome.unwrap_err().contains("允许范围"));
        assert_eq!(writes_of(&seen, crate::events::REPORT).len(), 0);
        assert_eq!(writes_of(&seen, crate::events::ERROR).len(), 1);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// 两个根同名时必须给出**彼此不同**的 label（本机真实情况：
    /// DocRenderer 的 document/ 与 Core 仓的 document/）。
    #[test]
    fn duplicate_tail_segments_are_disambiguated() {
        let roots = vec![
            PathBuf::from("/home/me/code/echo-tech/DocRenderer/document"),
            PathBuf::from("/home/me/.local/share/echo-agent-core/source/document"),
        ];
        let labels = disambiguated_labels(&roots);
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0], labels[1], "两个根不能给同一个名字: {labels:?}");
        // 最短能区分的后缀：一层不够就往上加一层。
        assert_eq!(labels[0], "DocRenderer/document");
        assert_eq!(labels[1], "source/document");
    }

    /// 唯一时不啰嗦：一个根只给末段，不要给整条路径。
    #[test]
    fn unique_tail_segment_stays_short() {
        let roots = vec![PathBuf::from("/srv/wiki")];
        assert_eq!(disambiguated_labels(&roots), vec!["wiki".to_string()]);
    }

    /// 三层才分开的情况也要处理（不能只往上加一层就放弃）。
    #[test]
    fn disambiguation_keeps_climbing_until_unique() {
        let roots = vec![PathBuf::from("/a/x/docs"), PathBuf::from("/b/x/docs")];
        let labels = disambiguated_labels(&roots);
        assert_ne!(labels[0], labels[1], "{labels:?}");
        assert_eq!(labels, vec!["a/x/docs".to_string(), "b/x/docs".to_string()]);
    }

    /// `hint` 用来确认路径：落在 HOME 下就缩写，否则原样给绝对路径。
    #[test]
    fn hint_uses_home_tilde() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(home) = home {
            let inside = home.join("code/docs");
            assert_eq!(home_relative(&inside), "~/code/docs");
            assert_eq!(home_relative(&home), "~");
        }
        let outside = PathBuf::from("/srv/docs");
        assert_eq!(home_relative(&outside), "/srv/docs");
    }

    /// 根列表事件里同时带 label 与 hint（面板两行显示靠它们）。
    #[tokio::test]
    async fn roots_event_carries_label_and_hint() {
        let dir = temp_dir("roots-hint");
        let root = dir.join("docs");
        let state = state_with(&root);
        let seen = collect_writes(&state);
        handle(&state, "ProDocRoots", json!({}))
            .await
            .expect("claimed")
            .expect("ok");
        let payload = seen
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| name == crate::events::ROOTS)
            .map(|(_, payload)| payload.clone())
            .expect("应有 prodoc.roots 事件");
        assert_eq!(payload["roots"][0]["label"], "docs");
        assert!(
            payload["roots"][0]["hint"]
                .as_str()
                .unwrap_or_default()
                .starts_with('/')
                || payload["roots"][0]["hint"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with('~'),
            "hint 应该是 ~ 缩写或绝对路径: {:?}",
            payload["roots"][0]["hint"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 根列表命令：事件里带生效上限（面板要显示"最多 2000 篇"这类说明）。
    #[tokio::test]
    async fn roots_command_emits_caps() {
        let dir = temp_dir("roots");
        let state = state_with(&dir.join("docs"));
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let seen = seen.clone();
            state.set_event_sink(std::sync::Arc::new(move |event| {
                seen.lock()
                    .unwrap()
                    .push((event.name.clone(), event.payload.clone()));
            }));
        }
        handle(&state, "ProDocRoots", json!({}))
            .await
            .expect("claimed")
            .expect("ok");
        let payload = seen
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| name == crate::events::ROOTS)
            .map(|(_, payload)| payload.clone())
            .expect("应有 prodoc.roots 事件");
        assert_eq!(payload["caps"]["max_files"], 2000);
        assert_eq!(payload["roots"][0]["label"], "docs");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
