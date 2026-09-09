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
    let out = Command::new("node")
        .arg("--version")
        .output()
        .map_err(|e| {
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

/// 已安装的 `@deepseek-ai/dsh` 版本（读安装包 package.json；缺失/损坏 = None）。
fn installed_version(dsh_dir: &Path) -> Option<String> {
    let manifest = dsh_dir.join("node_modules/@deepseek-ai/dsh/package.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// 写入/覆盖 `dsh/package.json` 与 `pnpm-workspace.yaml`，声明目标版本。
fn write_manifest(dsh_dir: &Path, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(dsh_dir).map_err(|e| format!("建 dsh 目录失败: {e}"))?;
    let manifest = serde_json::json!({
        "name": "dshr-dsh-runtime",
        "private": true,
        "type": "module",
        "dependencies": { "@deepseek-ai/dsh": version },
    });
    std::fs::write(
        dsh_dir.join("package.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("序列化 package.json 失败: {e}"))?,
    )
    .map_err(|e| format!("写 package.json 失败: {e}"))?;
    // 对齐官方 profile（packages/boot/app-boot/src/profile.ts 的 PROFILE_PNPM_WORKSPACE）：hoisted 平铺。
    std::fs::write(
        dsh_dir.join("pnpm-workspace.yaml"),
        "packages:\n  - .\n\nnodeLinker: hoisted\n",
    )
    .map_err(|e| format!("写 pnpm-workspace.yaml 失败: {e}"))?;
    Ok(())
}

/// 在 `dsh_dir` 执行 `pnpm install --force`，确保切换版本后 node_modules 真正重装。
fn pnpm_install(dsh_dir: &Path) -> Result<(), String> {
    let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    let mut cmd = Command::new(pnpm);
    cmd.args([
        "install",
        "--force",
        "--ignore-scripts",
        "--config.minimumReleaseAge=0",
    ]);
    if let Ok(store) = std::env::var("DSHR_PNPM_STORE") {
        if !store.is_empty() {
            cmd.arg(format!("--store-dir={store}"));
        }
    }
    let status = cmd
        .current_dir(dsh_dir)
        .status()
        .map_err(|e| format!("运行 {pnpm} install 失败（请确认 pnpm 在 PATH）: {e}"))?;
    if !status.success() {
        return Err(format!("pnpm install 失败（exit {status}）"));
    }
    Ok(())
}

/// 确保 `dsh_dir` 下的 `@deepseek-ai/dsh` 是 `version`：
/// - 版本匹配且 bin 存在 → 直接返回；
/// - 版本缺失/不匹配 → 重写 package.json 并 `pnpm install --force`，安装后再次校验。
/// 返回 bin 绝对路径。
pub fn ensure(dsh_dir: &Path, version: &str) -> Result<PathBuf, String> {
    ensure_node()?;
    let bin = dsh_dir.join(DSH_BIN_REL);
    let installed = installed_version(dsh_dir);
    if installed.as_deref() == Some(version) && bin.exists() {
        return Ok(bin);
    }

    write_manifest(dsh_dir, version)?;
    pnpm_install(dsh_dir)?;

    let installed = installed_version(dsh_dir);
    if installed.as_deref() != Some(version) {
        return Err(format!(
            "dsh runtime 版本不匹配：期望 {version}，实际 {}",
            installed
                .as_deref()
                .unwrap_or("未知（未读到安装包 package.json）")
        ));
    }
    if !bin.exists() {
        return Err(format!(
            "pnpm install 成功但 dsh bin 缺失（包结构变化？）: {}",
            bin.display()
        ));
    }
    Ok(bin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "dshr-runtime-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installed_version_reads_installed_package_json() {
        let dir = temp_dir("version");
        let package = dir.join("node_modules/@deepseek-ai/dsh");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{ "version": "0.1.5-alpha.1" }"#,
        )
        .unwrap();
        assert_eq!(installed_version(&dir).as_deref(), Some("0.1.5-alpha.1"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_manifest_pins_requested_version() {
        let dir = temp_dir("manifest");
        write_manifest(&dir, "0.1.5-alpha.1").unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest
                .get("dependencies")
                .and_then(|value| value.get("@deepseek-ai/dsh"))
                .and_then(serde_json::Value::as_str),
            Some("0.1.5-alpha.1")
        );
        assert!(dir.join("pnpm-workspace.yaml").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
