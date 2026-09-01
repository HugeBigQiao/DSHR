//! runtime 获取：确保 node + `@deepseek-ai/dsh` 可用（锁版本，`dsh/` 目录）。
//!
//! 目录设计（2026-09-01 定案，纠正 v3 早期把本体塞 data/ 的偏差）：
//!   `dsh/`           程序本体（发布不带；运行时检测/下载；删除可重下）
//!   `data/dsh-home/` runtime 的独立 HOME（profiles/sessions 等状态）
//!   `data/`          sqlite + settings（未来；dshr 与 dsh 各自的库都放这）
//!
//! 包管理器用 pnpm：共享全局 store 跨项目去重 + 默认不跑依赖脚本
//! （省掉 npm 对 @google/genai preinstall 等脚本的依赖），对齐官方 pnpm 栈。
//! `pnpm-workspace.yaml` 用 `nodeLinker: hoisted`（官方 profile 同款，解析与平铺一致）。
use std::path::{Path, PathBuf};
use std::process::Command;

/// dsh 安装目录下的 bin 入口（相对 `dsh/`）。
pub const DSH_BIN_REL: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";

/// node 最低版本（官方 engines：`^22.19 || >=24`）。
const NODE_MIN: (u32, u32) = (22, 19);

fn parse_node_version(output: &str) -> Option<(u32, u32)> {
    let v = output.trim().trim_start_matches('v');
    let mut parts = v.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// 检测 node：PATH 上有满足版本的 node 才继续；否则报清晰错误。
/// （自动安装 portable node 是下一步：缺失时下载官方 zip 到 `dsh/node-<ver>/`。）
pub fn ensure_node() -> Result<(), String> {
    let out = Command::new("node").arg("--version").output().map_err(|e| {
        format!(
            "未找到 node（dsh 需要 Node.js ≥{}.{}，请安装并加入 PATH，或等自动安装支持）: {e}",
            NODE_MIN.0, NODE_MIN.1
        )
    })?;
    let text = String::from_utf8_lossy(&out.stdout);
    let Some((major, minor)) = parse_node_version(&text) else {
        return Err(format!("node --version 输出无法解析: {text}"));
    };
    if (major, minor) >= NODE_MIN || major >= 24 {
        Ok(())
    } else {
        Err(format!(
            "node 版本过低: {}（需要 ≥{}.{}，推荐 24+）",
            text.trim(),
            NODE_MIN.0,
            NODE_MIN.1
        ))
    }
}

/// 确保 dsh 本体已装到 `dsh_dir`（`<workspace>/dsh`）；返回 bin 绝对路径（已存在则直接返回）。
/// store：`DSHR_PNPM_STORE` 非空时指到它（受控环境/测试用）；空 = pnpm 默认全局 store（正式桌面端共享去重）。
pub fn ensure(dsh_dir: &Path, version: &str) -> PathBuf {
    let bin = dsh_dir.join(DSH_BIN_REL);
    if bin.exists() {
        return bin;
    }
    std::fs::create_dir_all(dsh_dir).expect("建 dsh 目录");
    let manifest = serde_json::json!({
        "name": "dshr-dsh-runtime",
        "private": true,
        "type": "module",
        "dependencies": { "@deepseek-ai/dsh": version },
    });
    std::fs::write(
        dsh_dir.join("package.json"),
        serde_json::to_string_pretty(&manifest).expect("序列化 package.json"),
    )
    .expect("写 package.json");
    // 对齐官方 profile（packages/boot/app-boot/src/profile.ts 的 PROFILE_PNPM_WORKSPACE）：hoisted 平铺。
    // 2026-09-01 实测：node-pty/koffi 的 tarball 自带预编译产物（prebuilds/），
    // `--ignore-scripts` 安装后 runtime 完整可用——正式路线免 node-gyp/Python/MSVC 工具链。
    std::fs::write(
        dsh_dir.join("pnpm-workspace.yaml"),
        "packages:\n  - .\n\nnodeLinker: hoisted\n",
    )
    .expect("写 pnpm-workspace.yaml");

    // Windows 上 pnpm 是 .cmd shim，Rust Command 不做 PATHEXT 解析——显式带扩展名。
    // --ignore-scripts：跳过依赖构建脚本（原生模块走 tarball 预编译产物，见上注释）。
    // --config.minimumReleaseAge=0：pnpm 供应链年龄策略默认会拒绝刚发布的 alpha 包
    // （@deepseek-ai/* 每次发版都是新包，等够年龄不现实）——正式桌面端同样需要关掉。
    let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    let mut cmd = Command::new(pnpm);
    cmd.args([
        "install",
        "--ignore-scripts",
        "--config.minimumReleaseAge=0",
    ]);
    if let Ok(store) = std::env::var("DSHR_PNPM_STORE") {
        if !store.is_empty() {
            cmd.arg(format!("--store-dir={store}"));
        }
    }
    let status = cmd.current_dir(dsh_dir).status().expect("跑 pnpm install");
    assert!(status.success(), "pnpm install 失败（exit {status}）");
    assert!(bin.exists(), "pnpm 成功但 dsh bin 缺失（包结构变化？）");
    bin
}
