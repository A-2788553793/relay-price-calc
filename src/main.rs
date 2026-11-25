// 发布版不弹出黑色命令行窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod calc;

use calc::{compare, parse_number, Prices, Verdict};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("中转站计算工具")
            .with_inner_size([620.0, 480.0])
            .with_min_inner_size([520.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "中转站计算工具",
        options,
        Box::new(|cc| {
            setup_chinese_font(&cc.egui_ctx);
            Ok(Box::new(App::default()))
        }),
    )
}

/// 加载 Windows 自带的中文字体，否则中文会显示成方块。
fn setup_chinese_font(ctx: &egui::Context) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    let candidates = ["msyh.ttc", "simhei.ttf", "simsun.ttc"];
    let Some(bytes) = candidates
        .iter()
        .find_map(|f| std::fs::read(format!(r"{windir}\Fonts\{f}")).ok())
    else {
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("cjk".to_owned(), FontData::from_owned(bytes));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

const FIELDS: [&str; 5] = ["输入", "输出", "缓存写入", "缓存读取", "平均缓存率"];

#[derive(Default)]
struct ModelInput {
    /// 依次对应 FIELDS 的文本框内容
    values: [String; 5],
}

impl ModelInput {
    fn coefficient(&self) -> Result<f64, String> {
        let mut nums = [0.0; 5];
        for (i, text) in self.values.iter().enumerate() {
            nums[i] = parse_number(text).map_err(|_| format!("“{}”不是有效数字", FIELDS[i]))?;
        }
        Prices {
            input: nums[0],
            output: nums[1],
            cache_write: nums[2],
            cache_read: nums[3],
            cache_rate_percent: nums[4],
        }
        .coefficient()
    }
}

#[derive(Default)]
struct App {
    models: [ModelInput; 2],
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.heading("中转站计算工具");
            ui.label(
                RichText::new("价格填每百万 token 的单价，平均缓存率填百分比（如 80）。空着的格子按 0 计算。")
                    .weak(),
            );
            ui.add_space(6.0);

            let results: Vec<Result<f64, String>> =
                self.models.iter().map(ModelInput::coefficient).collect();

            ui.columns(2, |cols| {
                for (i, col) in cols.iter_mut().enumerate() {
                    model_panel(col, &format!("模型{}", i + 1), &mut self.models[i], &results[i]);
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.vertical_centered(|ui| verdict_view(ui, &results));
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                if ui.button("清空").clicked() {
                    self.models = Default::default();
                }
            });
        });
    }
}

fn model_panel(ui: &mut egui::Ui, title: &str, model: &mut ModelInput, result: &Result<f64, String>) {
    egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().size(18.0));
        egui::Grid::new(title).num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            for (i, name) in FIELDS.iter().enumerate() {
                ui.label(*name);
                ui.horizontal(|ui| {
                    let hint = if i == 4 { "0 ~ 100" } else { "0" };
                    ui.add(
                        egui::TextEdit::singleline(&mut model.values[i])
                            .hint_text(hint)
                            .desired_width(110.0),
                    );
                    if i == 4 {
                        ui.label("%");
                    }
                });
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        match result {
            Ok(c) => ui.label(RichText::new(format!("价格系数：{}", fmt(*c))).size(16.0)),
            Err(e) => ui.label(RichText::new(e).color(Color32::from_rgb(220, 80, 80))),
        };
    });
}

fn verdict_view(ui: &mut egui::Ui, results: &[Result<f64, String>]) {
    let (Ok(c1), Ok(c2)) = (&results[0], &results[1]) else {
        ui.label(RichText::new("请先修正上面标红的数值").size(18.0).weak());
        return;
    };
    let (c1, c2) = (*c1, *c2);
    let green = Color32::from_rgb(40, 170, 90);
    match compare(c1, c2) {
        Verdict::Equal => {
            ui.label(RichText::new("两个模型一样贵").size(24.0).strong());
        }
        v => {
            let (text, cheap, dear) = if v == Verdict::Model1Cheaper {
                ("模型1更便宜", c1, c2)
            } else {
                ("模型2更便宜", c2, c1)
            };
            ui.label(RichText::new(text).size(26.0).strong().color(green));
            // 便宜多少百分比（贵的那个不可能为 0，因为它比便宜的大）
            let saving = (dear - cheap) / dear * 100.0;
            ui.label(RichText::new(format!("便宜约 {saving:.1}%")).weak());
        }
    }
}

/// 最多保留 4 位小数，去掉末尾多余的 0；非常小的数用科学计数法，避免显示成 0。
fn fmt(v: f64) -> String {
    if v != 0.0 && v.abs() < 1e-4 {
        return format!("{v:.3e}");
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}
