use serde::{Deserialize, Deserializer, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MODEL_COUNT: usize = 4;
/// 顺序：输入、输出、缓存写入、缓存读取、平均缓存率。
pub const FIELD_COUNT: usize = 5;
pub const DEFAULT_FONT_SIZE: f32 = 15.0;
pub const MIN_FONT_SIZE: f32 = 11.0;
pub const MAX_FONT_SIZE: f32 = 24.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub name: String,
    #[serde(deserialize_with = "deserialize_values")]
    pub values: [String; FIELD_COUNT],
}

impl ModelConfig {
    pub fn new(index: usize) -> Self {
        Self {
            name: format!("模型{}", index + 1),
            values: std::array::from_fn(|_| String::new()),
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self::new(0)
    }
}

fn deserialize_values<'de, D>(deserializer: D) -> Result<[String; FIELD_COUNT], D::Error>
where
    D: Deserializer<'de>,
{
    let mut values = Vec::<String>::deserialize(deserializer)?.into_iter();
    Ok(std::array::from_fn(|_| values.next().unwrap_or_default()))
}

fn deserialize_models<'de, D>(deserializer: D) -> Result<[ModelConfig; MODEL_COUNT], D::Error>
where
    D: Deserializer<'de>,
{
    // 保留 name 是否缺失的信息，按模型位置给缺失名称设置正确的默认值。
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct ModelFields {
        name: Option<String>,
        #[serde(deserialize_with = "deserialize_values")]
        values: [String; FIELD_COUNT],
    }

    let mut models = Vec::<ModelFields>::deserialize(deserializer)?.into_iter();
    Ok(std::array::from_fn(|index| match models.next() {
        Some(model) => ModelConfig {
            name: model.name.unwrap_or_else(|| ModelConfig::new(index).name),
            values: model.values,
        },
        None => ModelConfig::new(index),
    }))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(deserialize_with = "deserialize_models")]
    pub models: [ModelConfig; MODEL_COUNT],
    pub dark_mode: bool,
    pub font: String,
    pub font_size: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            models: std::array::from_fn(ModelConfig::new),
            dark_mode: false,
            font: String::new(),
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl AppConfig {
    pub fn sanitize(&mut self) {
        if !self.font_size.is_finite() {
            self.font_size = DEFAULT_FONT_SIZE;
        }
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        for (index, model) in self.models.iter_mut().enumerate() {
            model.name = model.name.trim().chars().take(30).collect();
            if model.name.is_empty() {
                model.name = ModelConfig::new(index).name;
            }
        }
    }
}

/// %APPDATA%\relay-price-calc\config.json。
pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(|base| PathBuf::from(base).join("relay-price-calc").join("config.json"))
}

pub fn load() -> AppConfig {
    config_path().map_or_else(AppConfig::default, |path| load_from(&path))
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "无法获取 APPDATA 配置目录".to_string())?;
    save_to(&path, cfg)
}

pub fn load_from(path: &Path) -> AppConfig {
    let mut cfg: AppConfig = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    cfg.sanitize();
    cfg
}

// 无论写入、同步还是替换失败，都清理本次调用独占的临时文件。
struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 在目标目录写入完整的 pretty JSON，同步文件后原子替换目标。
pub fn save_to(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    let mut cfg = cfg.clone();
    cfg.sanitize();
    let json = serde_json::to_vec_pretty(&cfg).map_err(|err| format!("配置序列化失败：{err}"))?;
    let filename = path
        .file_name()
        .ok_or_else(|| "配置路径缺少文件名".to_string())?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|err| format!("创建配置目录失败：{err}"))?;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (pending, mut file) = loop {
        let mut name = std::ffi::OsString::from(".");
        name.push(filename);
        name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&temp_path) {
            Ok(file) => break (PendingFile(temp_path), file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("创建配置临时文件失败：{err}")),
        }
    };
    // Windows 上先关闭句柄，再 rename；std::fs::rename 支持覆盖现有文件。
    let written = file.write_all(&json).and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|err| format!("写入配置失败：{err}"))?;
    fs::rename(&pending.0, path).map_err(|err| format!("替换配置文件失败：{err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn defaults_have_four_distinct_models() {
        let cfg = AppConfig::default();
        assert!(!cfg.dark_mode);
        assert!(cfg.font.is_empty());
        assert_eq!(cfg.font_size, DEFAULT_FONT_SIZE);
        for (i, model) in cfg.models.iter().enumerate() {
            assert_eq!(*model, ModelConfig::new(i));
            assert!(model.values.iter().all(String::is_empty));
        }
        assert_eq!(ModelConfig::default(), ModelConfig::new(0));
    }

    #[test]
    fn serialization_preserves_input_text() {
        let mut cfg = AppConfig { dark_mode: true, font: "msyh.ttc".into(), font_size: 19.0, ..Default::default() };
        cfg.models[2].name = "自定义模型".into();
        cfg.models[2].values = [" ８０％ ", "invalid", "", "1e-3", "0.30"].map(String::from);
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<AppConfig>(&json).unwrap(), cfg);
    }

    #[test]
    fn missing_fields_keep_position_specific_defaults() {
        assert_eq!(serde_json::from_str::<AppConfig>("{}").unwrap(), AppConfig::default());
        let cfg: AppConfig = serde_json::from_str(r#"{"models":[{}, {"values":["7"]}], "dark_mode":true}"#).unwrap();
        assert!(cfg.dark_mode);
        assert_eq!(cfg.models[1].name, "模型2");
        assert_eq!(cfg.models[1].values, ["7", "", "", "", ""].map(String::from));
        assert_eq!(cfg.models[2], ModelConfig::new(2));
        assert_eq!(serde_json::from_str::<ModelConfig>("{}").unwrap(), ModelConfig::default());
    }

    #[test]
    fn arrays_are_padded_or_truncated() {
        let cfg: AppConfig = serde_json::from_str(r#"{"models":[{"name":"A","values":["1","2","3","4","5","6"]},{},{},{},{"name":"ignored"}]}"#).unwrap();
        assert_eq!(cfg.models[0].name, "A");
        assert_eq!(cfg.models[0].values, ["1", "2", "3", "4", "5"].map(String::from));
        assert_eq!(cfg.models[3].name, "模型4");
        let cfg: AppConfig = serde_json::from_str(r#"{"models":[]}"#).unwrap();
        assert_eq!(cfg, AppConfig::default());
        let model: ModelConfig = serde_json::from_str(r#"{"values":["a","b"]}"#).unwrap();
        assert_eq!(model.values, ["a", "b", "", "", ""].map(String::from));
    }

    #[test]
    fn sanitize_repairs_size_and_unicode_names_without_touching_inputs() {
        let mut cfg = AppConfig::default();
        cfg.models[0].name = " \t\n".into();
        cfg.models[1].name = "  新名称  ".into();
        cfg.models[2].name = "😀中".repeat(20);
        cfg.models[0].values[0] = "  ８０％ ".into();
        for (size, expected) in [(f32::NAN, 15.0), (f32::INFINITY, 15.0), (f32::NEG_INFINITY, 15.0), (0.0, 11.0), (100.0, 24.0), (17.5, 17.5)] {
            cfg.font_size = size;
            cfg.sanitize();
            assert_eq!(cfg.font_size, expected);
        }
        assert_eq!(cfg.models[0].name, "模型1");
        assert_eq!(cfg.models[1].name, "新名称");
        assert_eq!(cfg.models[2].name, "😀中".repeat(15));
        assert_eq!(cfg.models[0].values[0], "  ８０％ ");
    }

    #[test]
    fn load_handles_missing_broken_and_inaccessible_files() {
        let dir = TempDir::new();
        let path = dir.path().join("config.json");
        assert_eq!(load_from(&path), AppConfig::default());
        for invalid in ["{broken", "[]", r#"{"models":false}"#, r#"{"font_size":"bad"}"#] {
            fs::write(&path, invalid).unwrap();
            assert_eq!(load_from(&path), AppConfig::default());
        }
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(load_from(&path), AppConfig::default());
        assert_eq!(load_from(dir.path()), AppConfig::default());
        fs::write(&path, r#"{"models":[{"name":" "}],"font_size":999}"#).unwrap();
        let cfg = load_from(&path);
        assert_eq!(cfg.models[0].name, "模型1");
        assert_eq!(cfg.font_size, MAX_FONT_SIZE);
    }

    #[test]
    fn atomic_save_creates_parents_and_replaces_existing_config() {
        let dir = TempDir::new();
        let path = dir.path().join("nested/config.json");
        save_to(&path, &AppConfig::default()).unwrap();
        let mut cfg = AppConfig { dark_mode: true, ..Default::default() };
        cfg.models[3].values[1] = " 1.00 ".into();
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), cfg);
        assert!(fs::read_to_string(&path).unwrap().starts_with("{\n  \"models\""));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn save_sanitizes_a_copy() {
        let dir = TempDir::new();
        let mut cfg = AppConfig { font_size: f32::INFINITY, ..Default::default() };
        cfg.models[0].name = " ".into();
        let path = dir.path().join("config.json");
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), AppConfig::default());
        assert!(cfg.font_size.is_infinite());
        assert_eq!(cfg.models[0].name, " ");
    }

    #[test]
    fn failed_replacement_keeps_target_and_cleans_temporary_file() {
        let dir = TempDir::new();
        let target = dir.path().join("config.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "unchanged").unwrap();
        assert!(save_to(&target, &AppConfig::default()).is_err());
        assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "unchanged");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(save_to(&target.join("keep/child.json"), &AppConfig::default()).is_err());
    }

    #[test]
    fn concurrent_saves_leave_one_complete_config() {
        let dir = TempDir::new();
        let path = dir.path().join("config.json");
        std::thread::scope(|scope| {
            for i in 0..8 {
                let path = &path;
                scope.spawn(move || {
                    let mut cfg = AppConfig::default();
                    cfg.models[0].name = format!("writer{i}");
                    save_to(path, &cfg).unwrap();
                });
            }
        });
        let cfg: AppConfig = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(cfg.models[0].name.starts_with("writer"));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
