//! 落盘数据的 CSV 导出：现在用来**看数据**，将来是监控页的历史导出。
//!
//! 三个数据源（对应 `DESIGN.md` §8.1 的数据罗盘）：
//! 1. `dshr.db` 的六张事实表 → 一行一事实的 CSV（[`store_tables`]）；
//! 2. 某个会话的**内存/回放快照** → 会话历史（消息流 + 轮 + 工具，[`session_history`]）；
//! 3. `data/wire-logs/*.jsonl` → 按会话分组**回放**，跨会话地重建每个会话的历史
//!   （[`replay_sessions`]，这是「跨会话」能力的第一个落地点）。
//!
//! 为什么值得单独一个模块（而不是散在 store 或 UI 里）：导出是**纯函数**（输入库/快照，输出文本），
//! 不参与运行时链路；放在 `store` 里会让「写入语义」与「人想看什么」两类改动理由混在一起。
//! 入口暂时是门控测试（`tests/export_csv.rs`，`DSHR_EXPORT=1`）——因为本 crate 不提供可执行文件，
//! 而监控页尚未做；将来监控页直接调这里同一组函数。
//!
//! **为什么导出会话历史要回放 wire-log 而不是查库**：库里按设计**没有 messages 表**
//!（§8.2：不建 events 全量表，wire-log JSONL 已是 lossless 源），turns/tool_calls 只是聚合事实。
//! 所以「这个会话到底聊了什么」只有回放能得到；而回放用的 `Folder` 与在线是**同一套折叠语义**
//!（见 `tests/fold_projection.rs` 的同源同巡断言），因此导出与 UI 看到的不会分叉。
//!
//! 上接：`dshr-ui` 的监控页（未来）与 `tests/export_csv.rs`（真跑入口）。
//! 下接：`store::{Store, ExportTable}`、`fold::Folder`（回放）、`snapshot`（导出形状）、文件系统。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::fold::Folder;
use crate::snapshot::{MsgKind, SessionSnapshot};
use crate::store::{ExportTable, Result as StoreResult, Store};

/// 一个待写出的 CSV（表头 + 行 + 文件名）。
///
/// 为什么带上表头而不是只给 `String`：调用方（测试/监控页）要能报「写了哪些列、多少行」，
/// 而这些信息只有构造者知道；同时让 `to_csv` 保持纯粹（给定结构必然产出同样的文本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvFile {
    /// 文件名（不含 `.csv`）。
    pub name: String,
    /// 表头（列名）。
    pub headers: Vec<String>,
    /// 数据行（每行长度应与表头一致）。
    pub rows: Vec<Vec<String>>,
}

impl CsvFile {
    /// 数据行数（不含表头）。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 渲染成 CSV 文本（含表头；末尾带换行）。
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        out.push_str(&join_row(&self.headers));
        out.push('\n');
        for row in &self.rows {
            out.push_str(&join_row(row));
            out.push('\n');
        }
        out
    }
}

/// 一行 → CSV 行（逐字段转义）。
fn join_row(cells: &[String]) -> String {
    cells
        .iter()
        .map(|c| csv_field(c))
        .collect::<Vec<_>>()
        .join(",")
}

/// 单个字段的 CSV 转义：含逗号/引号/换行时加引号，内部引号翻倍（RFC 4180）。
///
/// 为什么要自己写而不是加个 csv crate：这是一个 6 行规则，而依赖树每加一个 crate 都要
/// 走一次供应链与版本维护（本项目已经因为 pnpm/node 依赖吃过教训）。
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// 导出 `dshr.db` 的全部事实表（§8.2 六张）。
pub fn store_tables(store: &Store) -> StoreResult<Vec<CsvFile>> {
    let mut files = Vec::new();
    for table in ExportTable::ALL {
        let (headers, rows) = store.export_rows(table)?;
        files.push(CsvFile {
            name: table.name().to_string(),
            headers,
            rows,
        });
    }
    Ok(files)
}

/// 一个会话的历史：消息流 + 轮 + 工具（三份 CSV）。
///
/// 为什么拆三份而不是一张宽表：消息、轮、工具是三种粒度（一行消息/一行轮/一行工具），
/// 合成宽表会把轮级与工具级字段塞进不相干的行里（多对多关系用宽表表达必然出现空洞）。
pub fn session_history(snap: &SessionSnapshot) -> Vec<CsvFile> {
    let sid = if snap.session_id.is_empty() {
        "unknown".to_string()
    } else {
        snap.session_id.clone()
    };
    let num = |v: u64| v.to_string();

    let messages = CsvFile {
        name: format!("{sid}.messages"),
        headers: [
            "seq",
            "time",
            "turn",
            "step",
            "kind",
            "source",
            "text",
            "reasoning",
            "error",
            "usage_input",
            "usage_output",
            "tool_call_id",
            "tool_name",
            "tool_duration_ms",
            "tool_is_error",
            "tool_arguments",
            "tool_result",
            "files_changed",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        rows: snap
            .messages
            .iter()
            .map(|m| {
                let tool = m.tool.as_ref();
                let diffs = tool
                    .map(|t| {
                        t.diffs
                            .iter()
                            .map(|d| format!("{} +{} -{}", d.path, d.added, d.removed))
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default();
                vec![
                    num(m.seq),
                    num(m.time),
                    m.turn.map(num).unwrap_or_default(),
                    m.step.map(num).unwrap_or_default(),
                    kind_name(m.kind).to_string(),
                    m.source.clone(),
                    m.text.clone(),
                    m.reasoning.clone().unwrap_or_default(),
                    m.error.clone().unwrap_or_default(),
                    m.usage
                        .as_ref()
                        .map(|u| num(u.input_tokens))
                        .unwrap_or_default(),
                    m.usage
                        .as_ref()
                        .map(|u| num(u.output_tokens))
                        .unwrap_or_default(),
                    tool.map(|t| t.call_id.clone()).unwrap_or_default(),
                    tool.map(|t| t.name.clone()).unwrap_or_default(),
                    tool.map(|t| num(t.duration_ms)).unwrap_or_default(),
                    tool.map(|t| if t.is_error { "1" } else { "0" })
                        .unwrap_or_default()
                        .to_string(),
                    tool.map(|t| t.arguments.clone()).unwrap_or_default(),
                    // 失败时优先给「原因」，成功时给结果正文（同一列，看 is_error 判定是哪种）。
                    tool.and_then(|t| t.error.clone())
                        .or_else(|| tool.and_then(|t| t.result.clone()))
                        .unwrap_or_default(),
                    diffs,
                ]
            })
            .collect(),
    };

    let turns = CsvFile {
        name: format!("{sid}.turns"),
        headers: [
            "turn",
            "started",
            "ended",
            "reason",
            "input",
            "output",
            "cache_read",
            "cache_write",
            "reasoning",
            "total",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        rows: snap
            .turns
            .iter()
            .map(|t| {
                vec![
                    num(t.turn),
                    t.start_time.map(num).unwrap_or_default(),
                    t.end_time.map(num).unwrap_or_default(),
                    t.reason.clone().unwrap_or_default(),
                    num(t.usage.input),
                    num(t.usage.output),
                    num(t.usage.cache_read),
                    num(t.usage.cache_write),
                    num(t.usage.reasoning),
                    num(t.usage.total),
                ]
            })
            .collect(),
    };

    vec![messages, turns]
}

/// 消息行种类 → CSV 里的稳定文本（不要用 Debug 格式：它随派生改动而变）。
fn kind_name(kind: MsgKind) -> &'static str {
    match kind {
        MsgKind::User => "user",
        MsgKind::Assistant => "assistant",
        MsgKind::Reasoning => "reasoning",
        MsgKind::Tool => "tool",
        MsgKind::Notice => "notice",
        MsgKind::Injected => "injected",
        MsgKind::Attempt => "attempt",
    }
}

/// 一行 wire-log 记录里的会话 id（只有 `cat="dsh"` 的通知才有）。
///
/// 返回 `None` 的行会被回放跳过：app 轨迹、请求/响应行、以及不属于任何会话的线级记录。
fn session_id_of(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("cat")?.as_str()? != "dsh" {
        return None;
    }
    value
        .get("raw")?
        .get("params")?
        .get("sessionId")?
        .as_str()
        .map(str::to_string)
}

/// 回放一个目录下的全部 wire-log，**按会话分组**重建历史。
///
/// 为什么必须分组：一个 wire-log 文件里可以并存多个会话的帧（同一 runtime 的多会话、
/// 血缘子会话），而 `Folder` 是**单会话**状态机——直接顺序喂进去会把两个会话混成一份历史。
/// 分组后每个会话一条独立时间线，这就是「跨会话」现在的落地形态：
/// 库里只有聚合，逐条历史在这里重建。
///
/// 排序：目录按文件名（`<epoch>-<pid>.jsonl`，天然时间序），文件内按行序。
/// 单个会话的事件因此在「跨文件」后仍是时间序（多次运行会各自留一个文件）。
pub fn replay_sessions(wire_log_dir: &Path) -> std::io::Result<BTreeMap<String, SessionSnapshot>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(wire_log_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();

    for path in paths {
        let text = std::fs::read_to_string(&path)?;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Some(sid) = session_id_of(line) {
                groups.entry(sid).or_default().push(line.to_string());
            }
        }
    }

    let mut out = BTreeMap::new();
    for (sid, lines) in groups {
        let mut folder = Folder::new();
        for line in &lines {
            // 单行损坏不该毁掉整段历史（回放是排查手段，不是校验器）——跳过继续。
            let _ = folder.push_wire_line(line);
        }
        out.insert(sid, folder.snapshot());
    }
    Ok(out)
}

/// 把一批 CSV 写到目录（不存在则创建），返回写出的路径。
///
/// 文件内容**带 UTF-8 BOM**：这些 CSV 是给人看的，而 Excel 与 Windows PowerShell 在
/// 没有 BOM 时会按本地 ANSI 解码 UTF-8 文本——中文直接变乱码（本项目在 `secrets.json`
/// 上踩过同一个坑的反面：**读**带 BOM 的 JSON 会解析失败，所以 BOM 只加在导出的文件里，
/// [`CsvFile::to_csv`] 保持纯文本）。
pub fn write_all(out_dir: &Path, files: &[CsvFile]) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let mut paths = Vec::new();
    for file in files {
        let path = out_dir.join(format!("{}.csv", file.name));
        std::fs::write(&path, format!("\u{feff}{}", file.to_csv()))?;
        paths.push(path);
    }
    Ok(paths)
}

/// 一次全量导出：库表 + 跨会话历史。
#[derive(Debug, Clone, Default)]
pub struct ExportReport {
    /// 写出的文件与各自的数据行数。
    pub files: Vec<(PathBuf, usize)>,
    /// 回放出来的会话数（每个会话产出一份 messages + 一份 turns）。
    pub sessions: usize,
}

/// 导出一套完整数据（库 + wire-log 历史）到目录。`store` 为 `None` 时跳过库表。
///
/// 为什么把两件事合成一个入口：它们是「看数据」的同一件事——只导库表会缺逐条历史，
/// 只导历史会缺聚合与审计；分开调用两次容易只做一半。
pub fn export_all(
    store: Option<&Store>,
    wire_log_dir: &Path,
    out_dir: &Path,
) -> std::io::Result<ExportReport> {
    let mut report = ExportReport::default();
    let mut files: Vec<CsvFile> = Vec::new();

    if let Some(store) = store {
        if let Ok(tables) = store_tables(store) {
            files.extend(tables);
        }
    }

    if wire_log_dir.exists() {
        if let Ok(sessions) = replay_sessions(wire_log_dir) {
            report.sessions = sessions.len();
            for (_, snap) in &sessions {
                files.extend(session_history(snap));
            }
        }
    }

    for path in write_all(out_dir, &files)? {
        let rows = files
            .iter()
            .find(|f| path.ends_with(format!("{}.csv", f.name)))
            .map(CsvFile::row_count)
            .unwrap_or(0);
        report.files.push((path, rows));
    }
    Ok(report)
}
