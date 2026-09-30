//! 落盘数据的 CSV 导出：现在用来**看数据**，将来是监控页的历史导出。
//!
//! 三个数据源（对应 `DESIGN.md` §8.1 的数据罗盘）：
//! 1. `dshr.db` 的八张事实表 → 一行一事实的 CSV（[`store_tables`]，表清单在 `ExportTable::ALL`）；
//! 2. 某个会话的**内存/回放快照** → 会话历史（消息流 + 轮 + 工具，[`session_history`]）；
//! 3. `data/wire-logs/*.jsonl` → 按会话分组**回放**，跨会话地重建每个会话的历史
//!   （[`replay_sessions`]，这是「跨会话」能力的第一个落地点）。
//!
//! 为什么值得单独一个模块（而不是散在 store 或 UI 里）：导出是**纯函数**（输入库/快照，输出文本），
//! 不参与运行时链路；放在 `store` 里会让「写入语义」与「人想看什么」两类改动理由混在一起。
//! 入口暂时是门控测试（`tests/export_csv.rs`，`DSHR_EXPORT=1`）——因为本 crate 不提供可执行文件，
//! 而监控页尚未做；将来监控页直接调这里同一组函数。
//!
//! **为什么导出会话历史走「回放 wire-log」而不是查库**（2026-09-29 起库里补了 `messages` 表，
//! 这条理由随之更新，**结论不变**）：库里装的是**引擎亲手跑过并落盘过的**会话
//!（实测：回放 `data/wire-logs` 得到 42 个会话时，库里只有 3 行 session），而 wire-log 是
//! **全部原始帧**——它既是旧历史（引擎落库之前的那些）的唯一来源，也是「重算/核对」的最终依据
//!（§8.2：不建 events 全量表，JSONL 才是 lossless 源）。回放用的 `Folder` 与在线是**同一套折叠
//! 语义**（`tests/fold_projection.rs` 有「同源同巡」断言），所以导出与 UI 看到的不会分叉；
//! 库里那份 `messages` 服务于另一件事：**聚合查询与「关掉再打开」的快速复原**。
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
        // 表头也走同一套转义（列名同样可能含逗号/引号）；每行末尾统一带换行，
        // 于是 `cat a.csv b.csv` 这类拼接不会把两行粘在一起。
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
    // 四种字符都要括引号：逗号（分列）、引号（转义语法）、`\n` 与 `\r`（分行的行分隔符）。
    // 消息正文里换行很常见，漏掉它会让**一行数据静默裂成两行**——CSV 导出最典型的损坏方式。
    // 内含引号按 RFC 4180 翻倍（`"` → `""`），而不是用反斜杠转义（后者不是 CSV 标准）。
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// 导出 `dshr.db` 的全部事实表（§8.2：八张，清单见 `ExportTable::ALL`）。
pub fn store_tables(store: &Store) -> StoreResult<Vec<CsvFile>> {
    // 逐表导出，列序由 `ExportTable` 显式给出（`export_rows` 自带稳定排序与列序）。
    // 为什么不用 `SELECT *`：列序会随 schema 迁移变化，而下游脚本/表格软件是按列位读的，
    // 列序一漂就成了**静默错位**（值还在，但含义换了）。
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

/// 一个会话的历史：**两份** CSV——`*.messages`（消息流，工具信息按列并入消息行）
/// 与 `*.turns`（轮结算）。
///
/// 为什么按粒度拆而不合成一张宽表：消息、轮是两种粒度（一行消息 / 一行轮），合成一张表会把
/// 轮级字段（起止/token）重复塞进该轮的每条消息行，或者反过来在只关心轮的地方留一堆空列。
/// 工具**没有**单独一份：一个工具调用与它的结果在 wire 上就是**同一条** `tool/result` 消息
///（`MsgItem::tool` 挂在消息行上），单独拆表反而要再造一个 join 键。
pub fn session_history(snap: &SessionSnapshot) -> Vec<CsvFile> {
    // 文件前缀用会话 id（一个目录里每个会话两份文件，靠前缀区分）。空 id 兜底成 `unknown`
    // 而不是报错：导出是排查手段，缺 id 的快照（刚启动还没握手之类）也该能落下来看。
    let sid = if snap.session_id.is_empty() {
        "unknown".to_string()
    } else {
        snap.session_id.clone()
    };
    // 数值列统一走 `to_string`：CSV 没有类型，**空串（未知/不适用）与 0（真的是 0）必须能区分**，
    // 所以下面每个数值字段都是 `Option` + `unwrap_or_default()`，而不是先 `unwrap_or(0)`。
    let num = |v: u64| v.to_string();

    // ① 消息流：一行 = 一条消息（`MsgItem`），工具调用/结果**横向铺成列**而不是另起一份表
    //（理由见函数头：工具信息本就挂在消息行上，拆表要造 join 键）。
    // 列序走「阅读顺序」——seq/time → turn/step → kind/source → 正文 → usage → 工具 → 文件；
    // 固定列序还有一个好处：两次导出的 diff 才有意义（列漂移会淹没真正的数据变化）。
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
                // 工具相关列先取出引用：`tool` 为 `None`（非工具行）时这些列全部落空串，
                // 于是「消息行」与「工具行」共用一张表，不需要 sentinel 值。
                let tool = m.tool.as_ref();
                // 文件改动折成 `路径 +增 -删` 的一格（多文件用 `; ` 分隔）：CSV 单元格里不适合再嵌
                // 结构，一格一字符串最便于在表格软件里按「哪些行改了文件」筛选。
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

    // ② 轮结算：一行 = 一轮（`TurnStat`），起止时间 + 结束原因 + token 六桶。这是做「耗时/成本」
    // 统计时的主表，与消息表的关联键是 `turn`（消息行也带 turn 列）。
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
    // 这里只在**分组**阶段调用（不在折叠路径上），所以宁可「解析一次取一个字段」也不用正则：
    // 帧是 JSON，正则在嵌套与转义上必错；而每行只解析一次、整目录也就几十 MB。
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    // `cat != "dsh"` 的行（app 轨迹、请求/响应）直接跳过：它们没有 `sessionId`，
    // 混进来只会造出一个永远折不出内容的假会话。
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
    // ① 分组容器用 `BTreeMap`（不是 `HashMap`）：会话 id 单调递增（`s-<epoch>`），有序输出
    // 让导出结果可复现、两次导出的 diff 有意义——排查工具的输出必须可比。
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // 文件名是 `<epoch>-<pid>.jsonl`，**字符串排序即时间序**（时间戳等宽），所以同一个会话
    // 跨多个文件（应用跑过多次就各留一个）时，折叠出来的顺序仍是真实时间序。
    let mut paths: Vec<PathBuf> = std::fs::read_dir(wire_log_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();

    // ② 一趟扫完所有文件，这里只**分组、不解析**：分组键取自信封的 `sessionId`。
    // 为什么不在这里顺手解析成事件：解析与折叠的语义只应有一处（`Folder`），这里多解一遍就等于
    // 养出「回放」与「在线」两套逻辑——那正是「同源同巡」断言要防的事。
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

    // ③ 每个会话一份**全新**的 `Folder`：`Folder` 是单会话状态机（见函数头「为什么必须分组」）。
    // 喂的是**原始 wire 行**而不是先解析的 `SessionEvent`——这正是在线那条路径（engine 也把原始行
    // 交给 Folder），于是回放与线上同源同巡。
    let mut out = BTreeMap::new();
    for (sid, lines) in groups {
        let mut folder = Folder::new();
        for line in &lines {
            // 单行损坏不该毁掉整段历史（回放是排查手段，不是校验器）——跳过继续。
            let _ = folder.push_wire_line(line);
        }
        // ④ 折叠完取快照即结果：形状与 UI 看到的一模一样，所以导出/统计不必再写一套渲染。
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
    // 目录由这里负责建（`create_dir_all` 幂等）：调用方给的是「导出到哪」，
    // 不该要求它先保证目录存在——多一层前置条件就多一种「什么都没导出来」的失败。
    std::fs::create_dir_all(out_dir)?;
    let mut paths = Vec::new();
    for file in files {
        let path = out_dir.join(format!("{}.csv", file.name));
        // BOM **只加在写出的文件里**，[`CsvFile::to_csv`] 保持纯文本：BOM 是给 Excel / PowerShell
        // 打开用的，进到别的程序里就是垃圾字符——本项目在 `secrets.json` 上踩过它的反面
        //（**读**带 BOM 的 JSON 会解析失败）。同一个字符，读侧与写侧的取舍刚好相反。
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

    // ① 库表（聚合 + 审计）。失败**不中断**：导出是排查手段，「拿到多少算多少」比「因为读库
    // 失败而两手空空」有用——库文件被占用/损坏时，wire-log 那半仍能把历史救出来。
    if let Some(store) = store {
        if let Ok(tables) = store_tables(store) {
            files.extend(tables);
        }
    }

    // ② 跨会话历史（逐条）。`wire_log_dir` 不存在就跳过——首次运行还没写过日志是正常状态，
    // 不是错误。每个会话展开成两份 CSV（messages / turns）。
    if wire_log_dir.exists() {
        if let Ok(sessions) = replay_sessions(wire_log_dir) {
            report.sessions = sessions.len();
            for (_, snap) in &sessions {
                files.extend(session_history(snap));
            }
        }
    }

    // ③ 一次写盘，再回填「每个文件多少行」：`write_all` 只回路径，行数按文件名回查
    //（文件名 = `CsvFile::name`，一一对应）。于是报告与「实际写出了什么」取自同一份数据，
    // 不会出现「报了没写」或「写了没报」。
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
