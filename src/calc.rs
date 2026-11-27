//! 价格系数计算逻辑（与界面无关，便于测试）。

/// 一个模型的价格（每百万 token）和平均缓存率（百分比 0~100）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
    /// 平均缓存率，百分比，例如 80 表示 80%。
    pub cache_rate_percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Model1Cheaper,
    Model2Cheaper,
    Equal,
}

/// 两个系数的相对差距小于这个比例就视为一样贵，只用来吸收浮点误差。
const RELATIVE_EPSILON: f64 = 1e-12;

impl Prices {
    /// 校验数值：都必须是有限、非负的数；缓存率必须在 0~100 之间。
    pub fn validate(&self) -> Result<(), String> {
        let fields = [
            ("输入", self.input),
            ("输出", self.output),
            ("缓存写入", self.cache_write),
            ("缓存读取", self.cache_read),
            ("平均缓存率", self.cache_rate_percent),
        ];
        for (name, v) in fields {
            if !v.is_finite() {
                return Err(format!("“{name}”不是有效数字"));
            }
            if v < 0.0 {
                return Err(format!("“{name}”不能是负数"));
            }
        }
        if self.cache_rate_percent > 100.0 {
            return Err("“平均缓存率”不能超过 100%".to_string());
        }
        Ok(())
    }

    /// 价格系数 = (输入 + 输出) × 未命中 + (缓存读取 + 缓存写入) × 缓存率
    /// 其中 未命中 = 1 − 缓存率。
    pub fn coefficient(&self) -> Result<f64, String> {
        self.validate()?;
        let hit = self.cache_rate_percent / 100.0;
        let miss = 1.0 - hit;
        let c = (self.input + self.output) * miss + (self.cache_read + self.cache_write) * hit;
        if !c.is_finite() {
            return Err("数值过大，无法计算".to_string());
        }
        Ok(c)
    }
}

pub fn compare(c1: f64, c2: f64) -> Verdict {
    let scale = c1.abs().max(c2.abs());
    if (c1 - c2).abs() <= scale * RELATIVE_EPSILON {
        Verdict::Equal
    } else if c1 < c2 {
        Verdict::Model1Cheaper
    } else {
        Verdict::Model2Cheaper
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub coefficient: f64,
}

/// 跳过 None，按价格系数排序；浮点误差内的并列模型保持输入顺序。
pub fn rank(coeffs: &[Option<f64>]) -> Vec<Ranked> {
    let mut ranked: Vec<_> = coeffs
        .iter()
        .enumerate()
        .filter_map(|(index, &coefficient)| {
            coefficient.map(|coefficient| Ranked { index, coefficient })
        })
        .collect();
    ranked.sort_by(|a, b| a.coefficient.total_cmp(&b.coefficient));

    // 相对误差相等不具传递性，不能直接用作 sort_by 的比较器。
    // 以每组最小值为基准归并近似相等项，避免误差链把明显不同的价格归为并列。
    // 再恢复组内的原始顺序。
    let mut start = 0;
    while start < ranked.len() {
        let mut end = start + 1;
        while end < ranked.len()
            && is_equal(ranked[start].coefficient, ranked[end].coefficient)
        {
            end += 1;
        }
        ranked[start..end].sort_by_key(|item| item.index);
        start = end;
    }
    ranked
}

/// 便宜的比贵的便宜多少百分比；贵的系数非正时返回 0。
pub fn saving_percent(cheap: f64, dear: f64) -> f64 {
    if dear <= 0.0 {
        0.0
    } else {
        (dear - cheap) / dear * 100.0
    }
}

/// 与 compare 返回 Equal 使用同一规则。
pub fn is_equal(a: f64, b: f64) -> bool {
    compare(a, b) == Verdict::Equal
}

/// 把用户输入的文本解析成数字。空白视为 0；支持全角数字和句号、末尾的 % 号。
pub fn parse_number(text: &str) -> Result<f64, String> {
    let normalized: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            '．' | '。' => '.',
            '％' => '%',
            _ => c,
        })
        .collect();
    let s = normalized.trim_end_matches('%').trim();
    if s.is_empty() {
        return Ok(0.0);
    }
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("“{}”不是有效数字", text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(input: f64, output: f64, w: f64, r: f64, rate: f64) -> Prices {
        Prices { input, output, cache_write: w, cache_read: r, cache_rate_percent: rate }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn example_from_design() {
        // (3 + 15)×0.2 + (0.3 + 3.75)×0.8 = 3.6 + 3.24 = 6.84
        let c = p(3.0, 15.0, 3.75, 0.3, 80.0).coefficient().unwrap();
        assert!(close(c, 6.84), "got {c}");
    }

    #[test]
    fn zero_cache_rate_uses_only_input_and_output() {
        let c = p(2.0, 8.0, 1.0, 0.5, 0.0).coefficient().unwrap();
        assert!(close(c, 2.0 + 8.0));
    }

    #[test]
    fn full_cache_rate_uses_only_read_and_write() {
        let c = p(2.0, 8.0, 1.0, 0.5, 100.0).coefficient().unwrap();
        assert!(close(c, 0.5 + 1.0));
    }

    #[test]
    fn all_zero_is_zero() {
        assert!(close(p(0.0, 0.0, 0.0, 0.0, 0.0).coefficient().unwrap(), 0.0));
    }

    #[test]
    fn rejects_bad_values() {
        assert!(p(-1.0, 0.0, 0.0, 0.0, 0.0).coefficient().is_err());
        assert!(p(1.0, 0.0, 0.0, 0.0, 100.1).coefficient().is_err());
        assert!(p(f64::NAN, 0.0, 0.0, 0.0, 0.0).coefficient().is_err());
        assert!(p(f64::INFINITY, 0.0, 0.0, 0.0, 0.0).coefficient().is_err());
    }

    #[test]
    fn compare_works() {
        assert_eq!(compare(1.0, 2.0), Verdict::Model1Cheaper);
        assert_eq!(compare(2.0, 1.0), Verdict::Model2Cheaper);
        assert_eq!(compare(0.1 + 0.2, 0.3), Verdict::Equal);
        assert_eq!(compare(0.0, 0.0), Verdict::Equal);
        // 极小但不为 0 的差距不能被当成一样贵
        assert_eq!(compare(0.0, 5e-10), Verdict::Model1Cheaper);
    }

    #[test]
    fn overflow_is_error() {
        assert!(p(1e308, 1e308, 0.0, 0.0, 0.0).coefficient().is_err());
    }

    #[test]
    fn parse_number_handles_common_inputs() {
        assert_eq!(parse_number("").unwrap(), 0.0);
        assert_eq!(parse_number("  ").unwrap(), 0.0);
        assert_eq!(parse_number("3.75").unwrap(), 3.75);
        assert_eq!(parse_number(" 80% ").unwrap(), 80.0);
        assert_eq!(parse_number("８０％").unwrap(), 80.0);
        assert_eq!(parse_number("０。５").unwrap(), 0.5);
        assert!(parse_number("abc").is_err());
        assert!(parse_number("inf").is_err());
        assert!(parse_number("NaN").is_err());
    }

    #[test]
    fn rank_sorts_and_skips_missing_models() {
        assert_eq!(
            rank(&[Some(3.0), None, Some(1.0), Some(2.0)]),
            vec![
                Ranked { index: 2, coefficient: 1.0 },
                Ranked { index: 3, coefficient: 2.0 },
                Ranked { index: 0, coefficient: 3.0 },
            ]
        );
        assert!(rank(&[]).is_empty());
        assert!(rank(&[None, None]).is_empty());
    }

    #[test]
    fn rank_preserves_order_for_exact_and_approximate_ties() {
        for values in [
            [0.3, 0.1 + 0.2],
            [0.1 + 0.2, 0.3],
            [1.0, 1.0],
            [1.0 + 5e-13, 1.0],
        ] {
            let ranked = rank(&values.map(Some));
            assert_eq!(ranked.iter().map(|r| r.index).collect::<Vec<_>>(), [0, 1]);
        }
        let ranked = rank(&[Some(0.1 + 0.2), Some(2.0), Some(0.3), Some(0.0)]);
        assert_eq!(ranked.iter().map(|r| r.index).collect::<Vec<_>>(), [3, 0, 2, 1]);
    }

    #[test]
    fn saving_percent_handles_boundaries() {
        assert!(close(saving_percent(1.0, 4.0), 75.0));
        assert_eq!(saving_percent(0.0, 1.0), 100.0);
        assert_eq!(saving_percent(1.0, 1.0), 0.0);
        assert_eq!(saving_percent(0.0, 0.0), 0.0);
        assert_eq!(saving_percent(1.0, -1.0), 0.0);
        assert_eq!(saving_percent(2.0, 1.0), -100.0);
    }

    #[test]
    fn rank_does_not_merge_a_chain_beyond_relative_tolerance() {
        let values = [1.0 + 1.5e-12, 1.0 + 0.75e-12, 1.0];
        assert!(!is_equal(values[0], values[2]));
        let ranked = rank(&values.map(Some));
        assert_eq!(ranked.last().unwrap().index, 0);
    }

    #[test]
    fn equality_matches_compare() {
        for (a, b) in [(0.3, 0.1 + 0.2), (0.0, 0.0), (0.0, 5e-10), (1.0, 2.0)] {
            assert_eq!(is_equal(a, b), compare(a, b) == Verdict::Equal);
        }
    }
}
