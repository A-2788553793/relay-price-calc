// 发布版不弹出黑色命令行窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, FontData, FontDefinitions, FontFamily, FontId, Layout, Margin, RichText,
    Rounding, Stroke, TextStyle, Theme, Vec2,
};
use relay_price_calc::calc::{parse_number, rank, saving_percent, Prices, Ranked};
use relay_price_calc::config::{
    self, AppConfig, ModelConfig, DEFAULT_FONT_SIZE, FIELD_COUNT, MAX_FONT_SIZE, MIN_FONT_SIZE,
    MODEL_COUNT,
};
use relay_price_calc::{fonts, shortcut};

const APP_TITLE: &str = "中转站计算工具";
const GITHUB_URL: &str = "https://github.com/A-2788553793/relay-price-calc";
const FIELDS: [&str; FIELD_COUNT] = ["输入", "输出", "缓存写入", "缓存读取", "平均缓存率"];
/// 输入停止这么久之后再写盘，避免每敲一个字就保存一次。
const SAVE_DELAY: Duration = Duration::from_millis(600);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([780.0, 800.0])
            .with_min_inner_size([640.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

// ───────────────────────────── 计算结果 ─────────────────────────────

/// 单个模型的计算状态。
enum ModelResult {
    /// 所有格子都空着，不参与比较。
    Empty,
    Ok(f64),
    Err(String),
}

fn evaluate(model: &ModelConfig) -> ModelResult {
    if model.values.iter().all(|v| v.trim().is_empty()) {
        return ModelResult::Empty;
    }
    let mut nums = [0.0; FIELD_COUNT];
    for (i, text) in model.values.iter().enumerate() {
        match parse_number(text) {
            Ok(v) => nums[i] = v,
            Err(_) => return ModelResult::Err(format!("“{}”不是有效数字", FIELDS[i])),
        }
    }
    let prices = Prices {
        input: nums[0],
        output: nums[1],
        cache_write: nums[2],
        cache_read: nums[3],
        cache_rate_percent: nums[4],
    };
    match prices.coefficient() {
        Ok(c) => ModelResult::Ok(c),
        Err(e) => ModelResult::Err(e),
    }
}

fn display_name(model: &ModelConfig, index: usize) -> String {
    let name = model.name.trim();
    if name.is_empty() {
        format!("模型{}", index + 1)
    } else {
        name.to_string()
    }
}

// ───────────────────────────── 配色 ─────────────────────────────

/// 跟随深浅主题变化的几种强调色。
struct Palette {
    accent: Color32,
    good: Color32,
    good_bg: Color32,
    error: Color32,
    card_bg: Color32,
    card_stroke: Color32,
}

impl Palette {
    fn of(dark: bool) -> Self {
        if dark {
            Palette {
                accent: Color32::from_rgb(110, 160, 255),
                good: Color32::from_rgb(90, 210, 130),
                good_bg: Color32::from_rgb(28, 52, 38),
                error: Color32::from_rgb(255, 110, 110),
                card_bg: Color32::from_rgb(36, 38, 44),
                card_stroke: Color32::from_rgb(58, 61, 70),
            }
        } else {
            Palette {
                accent: Color32::from_rgb(45, 105, 220),
                good: Color32::from_rgb(30, 150, 80),
                good_bg: Color32::from_rgb(232, 247, 237),
                error: Color32::from_rgb(210, 60, 60),
                card_bg: Color32::WHITE,
                card_stroke: Color32::from_rgb(222, 225, 232),
            }
        }
    }
}

/// 设置两套主题的基础外观（圆角、间距、背景色）。只需要在启动时调用一次。
fn setup_visuals(ctx: &egui::Context) {
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        let pal = Palette::of(dark);
        ctx.style_mut_of(theme, |style| {
            let v = &mut style.visuals;
            if dark {
                v.panel_fill = Color32::from_rgb(26, 27, 31);
                v.window_fill = Color32::from_rgb(34, 36, 41);
                v.extreme_bg_color = Color32::from_rgb(20, 21, 24);
            } else {
                v.panel_fill = Color32::from_rgb(244, 246, 250);
                v.window_fill = Color32::WHITE;
                v.extreme_bg_color = Color32::from_rgb(250, 251, 253);
            }
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.accent.gamma_multiply(0.35);
            v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
            v.window_rounding = Rounding::same(12.0);
            v.menu_rounding = Rounding::same(8.0);
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.rounding = Rounding::same(6.0);
            }
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(10.0, 4.0);
            style.spacing.interact_size.y = 26.0;
        });
    }
}

/// 按字号设置所有文字样式。
fn apply_font_size(ctx: &egui::Context, size: f32) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(size * 0.8)),
            (TextStyle::Body, FontId::proportional(size)),
            (TextStyle::Button, FontId::proportional(size)),
            (TextStyle::Monospace, FontId::monospace(size)),
            (TextStyle::Heading, FontId::proportional(size * 1.45)),
        ]
        .into();
    });
}

/// 加载中文字体（找不到任何中文字体时保持 egui 默认字体）。返回实际使用的字体文件名。
fn apply_font(ctx: &egui::Context, file: &str) -> Option<String> {
    let (used, bytes) = fonts::load_font(file)?;
    let mut defs = FontDefinitions::default();
    defs.font_data.insert("cjk".to_owned(), FontData::from_owned(bytes));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        defs.families.entry(family).or_default().insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(defs);
    Some(used)
}

// ───────────────────────────── 应用 ─────────────────────────────

struct App {
    cfg: AppConfig,
    /// 最近一次成功写盘的配置，用来判断是否有改动。
    saved: AppConfig,
    /// 第一次检测到未保存改动的时刻。
    dirty_since: Option<Instant>,
    save_error: Option<String>,

    /// 当前已加载到 egui 的字体与字号，变化时才重新设置。
    applied_font: Option<String>,
    applied_size: f32,
    font_in_use: Option<String>,
    font_choices: Vec<fonts::FontChoice>,

    show_settings: bool,
    show_about: bool,
    shortcut_msg: Option<Result<String, String>>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let cfg = config::load();
        setup_visuals(ctx);
        let mut app = App {
            saved: cfg.clone(),
            cfg,
            dirty_since: None,
            save_error: None,
            applied_font: None,
            applied_size: 0.0,
            font_in_use: None,
            font_choices: fonts::available_fonts(),
            show_settings: false,
            show_about: false,
            shortcut_msg: None,
        };
        app.sync_appearance(ctx);
        app
    }

    /// 把配置里的主题、字体、字号同步到 egui。
    fn sync_appearance(&mut self, ctx: &egui::Context) {
        ctx.set_theme(if self.cfg.dark_mode { Theme::Dark } else { Theme::Light });
        if self.applied_font.as_deref() != Some(self.cfg.font.as_str()) {
            self.font_in_use = apply_font(ctx, &self.cfg.font);
            self.applied_font = Some(self.cfg.font.clone());
        }
        if self.applied_size != self.cfg.font_size {
            apply_font_size(ctx, self.cfg.font_size);
            self.applied_size = self.cfg.font_size;
        }
    }

    /// 有改动时延迟保存；`force` 为 true 时立即保存（退出时用）。
    fn autosave(&mut self, ctx: &egui::Context, force: bool) {
        if self.cfg == self.saved {
            self.dirty_since = None;
            return;
        }
        let since = *self.dirty_since.get_or_insert_with(Instant::now);
        let elapsed = since.elapsed();
        if force || elapsed >= SAVE_DELAY {
            let mut to_save = self.cfg.clone();
            to_save.sanitize();
            match config::save(&to_save) {
                Ok(()) => {
                    self.save_error = None;
                    self.saved = self.cfg.clone();
                    self.dirty_since = None;
                }
                Err(e) => {
                    self.save_error = Some(e);
                    // 失败后过一会儿再试，不要每帧都写
                    self.dirty_since = Some(Instant::now());
                    ctx.request_repaint_after(SAVE_DELAY);
                }
            }
        } else {
            ctx.request_repaint_after(SAVE_DELAY - elapsed);
        }
    }

    fn pal(&self) -> Palette {
        Palette::of(self.cfg.dark_mode)
    }

    fn fs(&self, k: f32) -> f32 {
        self.cfg.font_size * k
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_appearance(ctx);

        let results: Vec<ModelResult> = self.cfg.models.iter().map(evaluate).collect();
        let coeffs: Vec<Option<f64>> = results
            .iter()
            .map(|r| match r {
                ModelResult::Ok(c) => Some(*c),
                _ => None,
            })
            .collect();
        let ranking = rank(&coeffs);

        self.top_bar(ctx);
        self.result_panel(ctx, &results, &ranking);
        self.models_area(ctx, &results, &ranking);
        self.settings_window(ctx);
        self.about_window(ctx);

        self.autosave(ctx, false);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 退出时把还没来得及写盘的改动保存掉
        if self.cfg != self.saved {
            let mut to_save = self.cfg.clone();
            to_save.sanitize();
            let _ = config::save(&to_save);
        }
    }
}

// ───────────────────────────── 界面各部分 ─────────────────────────────

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        let pal = self.pal();
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::none()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(Margin::symmetric(16.0, 10.0))
                    .stroke(Stroke::new(1.0_f32, pal.card_stroke)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(APP_TITLE).size(self.fs(1.35)).strong().color(pal.accent));
                    ui.label(RichText::new("看看哪家中转站更划算").weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.selectable_label(self.show_about, "关于").clicked() {
                            self.show_about = !self.show_about;
                        }
                        if ui.selectable_label(self.show_settings, "设置").clicked() {
                            self.show_settings = !self.show_settings;
                        }
                        let theme_label = if self.cfg.dark_mode { "浅色" } else { "深色" };
                        if ui
                            .button(theme_label)
                            .on_hover_text("切换深色 / 浅色主题")
                            .clicked()
                        {
                            self.cfg.dark_mode = !self.cfg.dark_mode;
                        }
                    });
                });
            });
    }

    fn models_area(&mut self, ctx: &egui::Context, results: &[ModelResult], ranking: &[Ranked]) {
        let pal = self.pal();
        let cheapest: Vec<usize> = cheapest_indices(ranking);
        let comparable = ranking.len() >= 2 && cheapest.len() < ranking.len();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(ctx.style().visuals.panel_fill).inner_margin(16.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "价格填每百万 token 的单价，平均缓存率填百分比（如 80）。\
                             全部留空的模型不参与比较，部分留空的格子按 0 计算。",
                        )
                        .weak()
                        .size(self.fs(0.9)),
                    );
                    ui.add_space(6.0);
                    let gap = 12.0;
                    let col_w = ((ui.available_width() - gap) / 2.0 - 1.0).max(200.0);
                    for row in 0..MODEL_COUNT / 2 {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for col in 0..2 {
                                let i = row * 2 + col;
                                let highlight = comparable && cheapest.contains(&i);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(col_w, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| self.model_card(ui, i, &results[i], highlight, &pal),
                                );
                            }
                        });
                        ui.add_space(gap - 8.0);
                    }
                });
            });
    }

    fn model_card(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        result: &ModelResult,
        highlight: bool,
        pal: &Palette,
    ) {
        let font_size = self.cfg.font_size;
        let model = &mut self.cfg.models[index];
        let (fill, stroke) = if highlight {
            (pal.good_bg, Stroke::new(1.5_f32, pal.good))
        } else {
            (pal.card_bg, Stroke::new(1.0_f32, pal.card_stroke))
        };
        egui::Frame::none()
            .fill(fill)
            .stroke(stroke)
            .rounding(12.0)
            .inner_margin(Margin::same(14.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // 标题行：可编辑的模型名 + 清除按钮
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let has_values = model.values.iter().any(|v| !v.is_empty());
                        if ui
                            .add_enabled(has_values, egui::Button::new("清除"))
                            .on_hover_text("清空这个模型填入的参数（保留名称）")
                            .clicked()
                        {
                            model.values = Default::default();
                        }
                        if highlight {
                            ui.label(RichText::new("最便宜").strong().color(pal.good));
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut model.name)
                                .font(FontId::proportional(font_size * 1.2))
                                .hint_text(format!("模型{}", index + 1))
                                .char_limit(30)
                                .desired_width(ui.available_width()),
                        )
                        .on_hover_text("点击修改名称，例如“某某中转站”");
                    });
                });
                ui.add_space(4.0);
                // 固定列宽，保证卡片内容不会把卡片撑出窗口
                let spacing = ui.spacing().item_spacing.x;
                let label_w = font_size * 5.4; // “平均缓存率”五个字
                let unit_w = font_size * 1.0;
                let edit_w = (ui.available_width() - label_w - unit_w - spacing * 2.0 - 8.0).max(60.0);
                for (i, name) in FIELDS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let is_rate = i == FIELD_COUNT - 1;
                        ui.allocate_ui_with_layout(
                            Vec2::new(label_w, ui.spacing().interact_size.y),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_min_width(label_w);
                                ui.label(*name);
                            },
                        );
                        let invalid = parse_number(&model.values[i]).is_err();
                        let mut edit = egui::TextEdit::singleline(&mut model.values[i])
                            .hint_text(if is_rate { "0 ~ 100" } else { "0" })
                            .desired_width(edit_w);
                        if invalid {
                            edit = edit.text_color(pal.error);
                        }
                        ui.add(edit);
                        if is_rate {
                            ui.label(RichText::new("%").weak());
                        }
                    });
                }
                ui.add_space(6.0);
                match result {
                    ModelResult::Empty => {
                        ui.label(RichText::new("未填写，不参与比较").weak());
                    }
                    ModelResult::Ok(c) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("价格系数").weak());
                            let color = if highlight { pal.good } else { ui.visuals().strong_text_color() };
                            ui.label(RichText::new(fmt(*c)).size(font_size * 1.3).strong().color(color));
                        });
                    }
                    ModelResult::Err(e) => {
                        ui.label(RichText::new(e).color(pal.error));
                    }
                }
            });
    }

    fn result_panel(&mut self, ctx: &egui::Context, results: &[ModelResult], ranking: &[Ranked]) {
        let pal = self.pal();
        egui::TopBottomPanel::bottom("result")
            .frame(
                egui::Frame::none()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(Margin::symmetric(20.0, 12.0))
                    .stroke(Stroke::new(1.0_f32, pal.card_stroke)),
            )
            .show(ctx, |ui| {
                let errors = results.iter().filter(|r| matches!(r, ModelResult::Err(_))).count();
                if ranking.len() < 2 {
                    ui.vertical_centered(|ui| {
                        let msg = if errors > 0 {
                            "请先修正标红的数值"
                        } else {
                            "至少填写两个模型才能比较"
                        };
                        ui.label(RichText::new(msg).size(self.fs(1.2)).weak());
                    });
                    return;
                }
                let names: Vec<String> = self
                    .cfg
                    .models
                    .iter()
                    .enumerate()
                    .map(|(i, m)| display_name(m, i))
                    .collect();
                let cheapest = cheapest_indices(ranking);
                let best = ranking[0].coefficient;

                ui.vertical_centered(|ui| {
                    let headline = if cheapest.len() == ranking.len() {
                        "参与比较的模型一样贵".to_string()
                    } else {
                        let who: Vec<&str> = cheapest.iter().map(|&i| names[i].as_str()).collect();
                        format!("{} 最便宜", who.join("、"))
                    };
                    let color = if cheapest.len() == ranking.len() {
                        ui.visuals().strong_text_color()
                    } else {
                        pal.good
                    };
                    ui.label(RichText::new(headline).size(self.fs(1.6)).strong().color(color));
                });
                ui.add_space(4.0);

                // 排名列表
                let mut place = 0;
                let mut prev: Option<f64> = None;
                egui::Grid::new("ranking")
                    .num_columns(4)
                    .spacing([18.0, 4.0])
                    .show(ui, |ui| {
                        for (n, r) in ranking.iter().enumerate() {
                            // 并列的名次相同
                            if prev.map_or(true, |p| !relay_price_calc::calc::is_equal(p, r.coefficient)) {
                                place = n + 1;
                            }
                            prev = Some(r.coefficient);
                            let is_best = relay_price_calc::calc::is_equal(r.coefficient, best);
                            let color = if is_best { pal.good } else { ui.visuals().text_color() };
                            ui.label(RichText::new(format!("第 {place} 名")).color(color));
                            ui.label(RichText::new(&names[r.index]).strong().color(color));
                            ui.label(format!("系数 {}", fmt(r.coefficient)));
                            if is_best {
                                ui.label(RichText::new("—").weak());
                            } else {
                                let s = saving_percent(best, r.coefficient);
                                ui.label(
                                    RichText::new(format!("最便宜的比它便宜约 {s:.1}%")).weak(),
                                );
                            }
                            ui.end_row();
                        }
                    });
                if errors > 0 {
                    ui.label(
                        RichText::new(format!("有 {errors} 个模型数值有误，未参与比较"))
                            .color(pal.error)
                            .size(self.fs(0.85)),
                    );
                }
                if let Some(e) = &self.save_error {
                    ui.label(
                        RichText::new(format!("自动保存失败：{e}")).color(pal.error).size(self.fs(0.85)),
                    );
                }
            });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let pal = self.pal();
        egui::Window::new("设置")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(self.fs(22.0))
            .show(ctx, |ui| {
                section(ui, "外观");
                egui::Grid::new("settings").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    ui.label("主题");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.cfg.dark_mode, false, "浅色");
                        ui.selectable_value(&mut self.cfg.dark_mode, true, "深色");
                    });
                    ui.end_row();

                    ui.label("字体");
                    let current = if self.cfg.font.is_empty() {
                        "自动".to_string()
                    } else {
                        self.font_choices
                            .iter()
                            .find(|f| f.file == self.cfg.font)
                            .map(|f| f.label.to_string())
                            .unwrap_or_else(|| self.cfg.font.clone())
                    };
                    egui::ComboBox::from_id_salt("font")
                        .selected_text(current)
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cfg.font, String::new(), "自动");
                            for f in &self.font_choices {
                                ui.selectable_value(&mut self.cfg.font, f.file.to_string(), f.label);
                            }
                        });
                    ui.end_row();

                    ui.label("字号");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.cfg.font_size, MIN_FONT_SIZE..=MAX_FONT_SIZE)
                                .step_by(1.0)
                                .fixed_decimals(0),
                        );
                        if ui
                            .add_enabled(
                                self.cfg.font_size != DEFAULT_FONT_SIZE,
                                egui::Button::new("默认"),
                            )
                            .clicked()
                        {
                            self.cfg.font_size = DEFAULT_FONT_SIZE;
                        }
                    });
                    ui.end_row();
                });
                if !self.cfg.font.is_empty()
                    && self.font_in_use.as_deref() != Some(self.cfg.font.as_str())
                    && self.applied_font.as_deref() == Some(self.cfg.font.as_str())
                {
                    ui.label(RichText::new("所选字体无法加载，已改用其他字体").color(pal.error));
                }

                ui.add_space(8.0);
                section(ui, "快捷方式");
                if ui.button(format!("在桌面创建“{}”快捷方式", shortcut::SHORTCUT_NAME)).clicked() {
                    self.shortcut_msg = Some(
                        shortcut::create_desktop_shortcut()
                            .map(|p| format!("已创建：{}", p.display())),
                    );
                }
                match &self.shortcut_msg {
                    Some(Ok(m)) => {
                        ui.label(RichText::new(m).color(pal.good));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("创建失败：{e}")).color(pal.error));
                    }
                    None => {}
                }
                ui.add_space(4.0);
                ui.label(RichText::new("输入的数值和设置会自动保存。").weak().size(self.fs(0.85)));
            });
        self.show_settings = open;
        if !open {
            self.shortcut_msg = None;
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        let pal = self.pal();
        egui::Window::new("关于")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(self.fs(20.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(APP_TITLE).size(self.fs(1.5)).strong().color(pal.accent));
                    ui.label(RichText::new(format!("版本 {}", env!("CARGO_PKG_VERSION"))).weak());
                    ui.add_space(6.0);
                    ui.label("对比多个模型在中转站上的价格，算出谁更划算。");
                    ui.add_space(6.0);
                    ui.hyperlink_to("GitHub 项目主页", GITHUB_URL);
                    ui.label(RichText::new(GITHUB_URL).weak().size(self.fs(0.8)));
                    ui.add_space(6.0);
                    ui.label(RichText::new("基于 MIT 许可证开源").weak().size(self.fs(0.85)));
                });
            });
        self.show_about = open;
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// 排名第一且与第一名一样贵的所有模型下标。
fn cheapest_indices(ranking: &[Ranked]) -> Vec<usize> {
    let Some(first) = ranking.first() else {
        return Vec::new();
    };
    ranking
        .iter()
        .take_while(|r| relay_price_calc::calc::is_equal(r.coefficient, first.coefficient))
        .map(|r| r.index)
        .collect()
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
