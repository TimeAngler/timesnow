use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 扫描 assets 下的子目录，把文件名（去扩展名）当作 sprite 名
pub fn scan_all(base: &Path) -> HashMap<String, PathBuf> {
    let mut result = HashMap::new();

    for subdir in ["bg", "character", "ui"] {
        let dir = base.join("assets").join(subdir);
        if let Err(e) = scan_dir(&dir, None, &mut result) {
            eprintln!("扫描 {} 失败: {e}", dir.display());
        }
    }

    result
}

fn scan_dir(
    dir: &Path,
    prefix: Option<&str>,        // 上一层的目录名
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
        if !matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" ) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };

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

/// 从 assets.ron 读取 sprite 名 → 文件路径的映射
pub fn load_table(path: &Path) -> HashMap<String, String> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读取 assets.ron 失败: {e}"));
    ron::from_str(&content)
        .unwrap_or_else(|e| panic!("解析 assets.ron 失败: {e}"))
}

/// 按资源表逐个读文件，返回 (name, bytes)
pub fn load_all(table: &HashMap<String, String>, base: &Path) -> Vec<(String, Vec<u8>)> {
    table
        .iter()
        .map(|(name, rel)| {
            let full = base.join(rel);
            let bytes = std::fs::read(&full)
                .unwrap_or_else(|e| panic!("加载资源 {name} ({}) 失败: {e}", full.display()));
            (name.clone(), bytes)
        }).collect()
}