use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 扫描 assets 下的子目录，把文件名（去扩展名）当作 sprite 名
pub fn scan_all(base: &Path) -> HashMap<String, PathBuf> {
    let mut result = HashMap::new();
    let assets = base.join("assets");

    for subdir in ["bg", "character", "ui"] {
        let dir = assets.join(subdir);
        if let Err(e) = scan_dir(&dir, None, &mut result) {
            eprintln!("扫描 {} 失败: {e}", dir.display());
        }
    }

    // 兼容早期文档里写的 assets/char/。只在 character/ 不存在时生效，
    // 避免同一个 sprite 名在两个目录里各有一份、谁赢看运气。
    let legacy = assets.join("char");
    if !assets.join("character").exists() && legacy.exists() {
        eprintln!("提示：assets/char/ 已更名为 assets/character/，本次仍按旧名加载");
        if let Err(e) = scan_dir(&legacy, None, &mut result) {
            eprintln!("扫描 {} 失败: {e}", legacy.display());
        }
    }

    result
}

fn scan_dir(
    dir: &Path,
    prefix: Option<&str>, // 上一层的目录名
    result: &mut HashMap<String, PathBuf>,
) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // 递归，把目录名作为前缀
            let sub_name = path.file_name().and_then(|s| s.to_str());
            let new_prefix = match (prefix, sub_name) {
                (Some(p), Some(s)) => Some(format!("{p}_{s}")),
                (None, Some(s)) => Some(s.to_string()),
                _ => prefix.map(|s| s.to_string()),
            };
            scan_dir(&path, new_prefix.as_deref(), result)?;
            continue;
        }

        if !path.is_file() {
            continue;
        }

        // 只认图片
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        ) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        // 拼接前缀
        let name = match prefix {
            Some(p) => format!("{p}_{stem}"),
            None => stem.to_string(),
        };

        if result.contains_key(&name) {
            eprintln!("警告：sprite 名 '{name}' 重复，{} 被跳过", path.display());
            continue;
        }

        result.insert(name, path.clone());
    }
    Ok(())
}
