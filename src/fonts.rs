use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct FontChoice {
    pub label: &'static str,
    pub file: &'static str,
}

pub const CANDIDATES: &[FontChoice] = &[
    FontChoice { label: "微软雅黑", file: "msyh.ttc" },
    FontChoice { label: "等线", file: "Deng.ttf" },
    FontChoice { label: "黑体", file: "simhei.ttf" },
    FontChoice { label: "宋体", file: "simsun.ttc" },
    FontChoice { label: "楷体", file: "simkai.ttf" },
    FontChoice { label: "仿宋", file: "simfang.ttf" },
];

fn fonts_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into()))
        .join("Fonts")
}

pub fn available_fonts() -> Vec<FontChoice> {
    available_fonts_in(&fonts_dir())
}

fn available_fonts_in(dir: &Path) -> Vec<FontChoice> {
    CANDIDATES
        .iter()
        .filter(|font| dir.join(font.file).is_file())
        .cloned()
        .collect()
}

fn valid_filename(file: &str) -> bool {
    !file.is_empty()
        && file != "."
        && !file.contains("..")
        && !file.chars().any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
}

/// 优先读取指定的纯文件名，失败时依候选顺序回退。
pub fn load_font(file: &str) -> Option<(String, Vec<u8>)> {
    load_font_from(&fonts_dir(), file)
}

fn load_font_from(dir: &Path, file: &str) -> Option<(String, Vec<u8>)> {
    if valid_filename(file) {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            return Some((file.to_owned(), bytes));
        }
    }
    CANDIDATES.iter().find_map(|font| {
        std::fs::read(dir.join(font.file))
            .ok()
            .map(|bytes| (font.file.to_owned(), bytes))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn candidates_have_the_required_names_and_order() {
        assert_eq!(CANDIDATES.iter().map(|f| (f.label, f.file)).collect::<Vec<_>>(), [
            ("微软雅黑", "msyh.ttc"), ("等线", "Deng.ttf"), ("黑体", "simhei.ttf"),
            ("宋体", "simsun.ttc"), ("楷体", "simkai.ttf"), ("仿宋", "simfang.ttf"),
        ]);
    }

    #[test]
    fn enumeration_checks_files_and_preserves_candidate_order() {
        let dir = TempDir::new();
        assert!(available_fonts_in(dir.path()).is_empty());
        std::fs::write(dir.path().join("simkai.ttf"), b"kai").unwrap();
        std::fs::write(dir.path().join("Deng.ttf"), b"deng").unwrap();
        std::fs::create_dir(dir.path().join("msyh.ttc")).unwrap();
        assert_eq!(available_fonts_in(dir.path()), [CANDIDATES[1].clone(), CANDIDATES[4].clone()]);
    }

    #[test]
    fn loading_prefers_selection_then_first_readable_candidate() {
        let dir = TempDir::new();
        assert_eq!(load_font_from(dir.path(), "missing.ttf"), None);
        std::fs::create_dir(dir.path().join("msyh.ttc")).unwrap();
        std::fs::write(dir.path().join("Deng.ttf"), b"fallback").unwrap();
        std::fs::write(dir.path().join("custom.ttf"), b"selected").unwrap();
        assert_eq!(load_font_from(dir.path(), "custom.ttf"), Some(("custom.ttf".into(), b"selected".to_vec())));
        for file in ["", "missing.ttf", "msyh.ttc"] {
            assert_eq!(load_font_from(dir.path(), file), Some(("Deng.ttf".into(), b"fallback".to_vec())));
        }
    }

    #[test]
    fn traversal_and_windows_path_forms_are_rejected() {
        for file in ["", ".", "..", "../outside.ttf", "..\\outside.ttf", "/outside.ttf", "C:\\outside.ttf", "C:outside.ttf", "sub/font.ttf", "sub\\font.ttf", "font..ttf", "font.ttf:stream", "bad\0.ttf"] {
            assert!(!valid_filename(file), "不应接受 {file:?}");
        }
        assert!(valid_filename("custom.ttf"));
        let dir = TempDir::new();
        let fonts = dir.path().join("Fonts");
        std::fs::create_dir(&fonts).unwrap();
        std::fs::write(dir.path().join("outside.ttf"), b"outside").unwrap();
        assert_eq!(load_font_from(&fonts, "../outside.ttf"), None);
        std::fs::write(fonts.join("msyh.ttc"), b"safe").unwrap();
        for file in ["../outside.ttf", "..\\outside.ttf", "sub/font.ttf"] {
            assert_eq!(load_font_from(&fonts, file), Some(("msyh.ttc".into(), b"safe".to_vec())));
        }
    }
}
