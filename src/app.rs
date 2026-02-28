use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use crate::translate::{translate_text, Language};
use tray_icon::{TrayIcon, TrayIconBuilder, Icon, TrayIconEvent, MouseButton, MouseButtonState};
use tray_icon::menu::{Menu, MenuItem, MenuEvent, MenuId};

// ── 配置持久化 ──────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AppConfig {
    #[serde(default)]
    api_key: String,
}

fn config_path() -> std::path::PathBuf {
    // 保存到 exe 所在目录的 config 子文件夹下
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = exe_dir.join("config");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

fn load_config() -> AppConfig {
    let path = config_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, json);
    }
}

enum AppEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

pub struct TranslateApp {
    input_text: String,
    output_text: String,
    source_lang: Language,
    target_lang: Language,
    is_translating: bool,
    api_key: String,
    translation_rx: Option<Receiver<Result<String, String>>>,
    translation_tx: Sender<Result<String, String>>,

    show_settings: bool,
    api_key_visible: bool,

    _tray_icon: TrayIcon,
    /// 逻辑上是否「可见」（true = 在屏幕上，false = 移出屏幕外）
    show_window: bool,
    /// 窗口在屏幕上时的位置，用于隐藏后恢复
    window_pos: egui::Pos2,
    is_pinned: bool,

    app_rx: Receiver<AppEvent>,
    quit_id: MenuId,
}

fn load_icon() -> Icon {
    // 在编译期将 ico 文件嵌入二进制，运行时直接解码为 RGBA，无需外部文件
    let ico_bytes = include_bytes!("assets/icons/ico.ico");
    let img = image::load_from_memory(ico_bytes)
        .expect("failed to decode ico")
        .into_rgba8();
    let (width, height) = img.dimensions();
    Icon::from_rgba(img.into_raw(), width, height).expect("failed to create tray icon")
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // 尝试加载系统默认中文字体（微软雅黑）
    fonts.font_data.insert(
        "msyh".to_owned(),
        egui::FontData::from_static(include_bytes!("C:\\Windows\\Fonts\\msyh.ttc")).into(),
    );

    // 将中文字体设置为首选字体
    fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
        .insert(0, "msyh".to_owned());
    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap()
        .insert(0, "msyh".to_owned());

    ctx.set_fonts(fonts);
}

impl TranslateApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (app_tx, app_rx) = std::sync::mpsc::channel();
        
        // 设置中文字体
        setup_custom_fonts(&cc.egui_ctx);
        
        let tray_menu = Menu::new();
        let quit_i = MenuItem::new("退出", true, None);
        let quit_id = quit_i.id().clone();
        let _ = tray_menu.append(&quit_i);

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("Translate Dock")
            .with_icon(load_icon())
            .with_menu(Box::new(tray_menu))
            .build()
            .unwrap();

        // 启动后台线程监听托盘事件，唤醒 UI
        let ctx_clone1 = cc.egui_ctx.clone();
        let app_tx_clone1 = app_tx.clone();
        let tray_rx = TrayIconEvent::receiver().clone();
        std::thread::spawn(move || {
            while let Ok(event) = tray_rx.recv() {
                let _ = app_tx_clone1.send(AppEvent::Tray(event));
                ctx_clone1.request_repaint();
            }
        });

        let ctx_clone2 = cc.egui_ctx.clone();
        let app_tx_clone2 = app_tx.clone();
        let menu_rx = MenuEvent::receiver().clone();
        std::thread::spawn(move || {
            while let Ok(event) = menu_rx.recv() {
                let _ = app_tx_clone2.send(AppEvent::Menu(event));
                ctx_clone2.request_repaint();
            }
        });

        // 自定义 egui 视觉效果，实现现代圆润风格 (浅色主题)
        let mut visuals = egui::Visuals::light();
        visuals.window_corner_radius = egui::CornerRadius::same(16);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);

        // 浅色主题的阴影
        visuals.window_shadow = egui::Shadow {
            offset: [0i8, 4i8],
            blur: 12u8,
            spread: 0u8,
            color: egui::Color32::from_black_alpha(40),
        };
        
        // 调整背景颜色，使其更柔和
        visuals.window_fill = egui::Color32::from_rgb(250, 250, 250);
        visuals.panel_fill = egui::Color32::from_rgb(250, 250, 250);
        
        cc.egui_ctx.set_visuals(visuals);

        // 启动时从配置文件加载持久化数据
        let config = load_config();

        Self {
            input_text: String::new(),
            output_text: String::new(),
            source_lang: Language::Chinese,
            target_lang: Language::English,
            is_translating: false,
            api_key: config.api_key,   // ← 从配置文件恢复
            translation_rx: Some(rx),
            translation_tx: tx,
            show_settings: false,
            api_key_visible: false,
            _tray_icon: tray_icon,
            show_window: true,
            // 默认位置：屏幕右下角附近，适合 1080p 及以上分辨率
            window_pos: egui::pos2(1400.0, 600.0),
            is_pinned: true,
            app_rx,
            quit_id,
        }
    }
    
    fn trigger_translation(&mut self, ctx: &egui::Context) {
        if self.input_text.trim().is_empty() {
            return;
        }
        self.is_translating = true;
        self.output_text = "翻译中...".to_string();
        
        let tx = self.translation_tx.clone();
        let text = self.input_text.clone();
        let src = self.source_lang.clone();
        let tgt = self.target_lang.clone();
        let key = self.api_key.clone();
        let ctx_clone = ctx.clone();

        tokio::spawn(async move {
            let result = translate_text(&text, src, tgt, &key).await;
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }
}

impl eframe::App for TranslateApp {
    // 告诉 eframe 窗口背景是透明的
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    // 应用退出时保存配置（兜底，正常情况下 changed() 已实时保存）
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_config(&AppConfig { api_key: self.api_key.clone() });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 追踪窗口当前位置，以便「隐藏」后能复原
        // 仅在窗口可见时更新，避免保存到屏幕外的坐标
        if self.show_window {
            if let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect) {
                // 只有在合理范围内才更新（排除移出屏幕外时的位置）
                if outer_rect.min.x > -5000.0 {
                    self.window_pos = outer_rect.min;
                }
            }
        }

        // 处理失去焦点：非固定时移出屏幕外
        if !self.is_pinned && self.show_window {
            let lost_focus = ctx.input(|i| i.events.iter().any(|e| {
                matches!(e, egui::Event::WindowFocused(false))
            }));
            if lost_focus {
                self.show_window = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    egui::pos2(-32000.0, -32000.0)
                ));
            }
        }

        // 处理托盘和菜单事件
        while let Ok(event) = self.app_rx.try_recv() {
            match event {
                AppEvent::Tray(tray_event) => {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = tray_event
                    {
                        if self.show_window {
                            // 当前可见 → 移出屏幕外
                            self.show_window = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                egui::pos2(-32000.0, -32000.0)
                            ));
                        } else {
                            // 当前隐藏 → 移回屏幕
                            self.show_window = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                self.window_pos
                            ));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                    }
                }
                AppEvent::Menu(menu_event) => {
                    if menu_event.id == self.quit_id {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }

        // 处理翻译结果
        if let Some(rx) = &self.translation_rx {
            if let Ok(result) = rx.try_recv() {
                self.is_translating = false;
                match result {
                    Ok(text) => self.output_text = text,
                    Err(err) => self.output_text = format!("错误: {}", err),
                }
            }
        }

        // 主 UI 布局
        // 修复底部直角问题：CentralPanel 默认会填满整个窗口，如果内部内容没有撑满，
        // 底部可能会显示为直角。我们需要确保 Frame 的圆角应用到整个面板，
        // 并且添加 outer_margin 给阴影留出空间，防止阴影和圆角被窗口边缘裁剪。
        let frame = egui::Frame::new()
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(16.0)
            .outer_margin(16.0)
            .fill(ctx.style().visuals.window_fill())
            .shadow(ctx.style().visuals.window_shadow);

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // 强制 UI 填满整个可用空间，确保底部圆角被正确渲染
            ui.set_min_height(ui.available_height());
            
            // 顶部栏
            ui.horizontal(|ui| {
                ui.heading("翻译");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").on_hover_text("设置").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    
                    let pin_icon = if self.is_pinned { "📌" } else { "📍" };
                    let pin_tooltip = if self.is_pinned { "取消固定（失去焦点自动隐藏）" } else { "固定（始终在前台）" };
                    if ui.button(pin_icon).on_hover_text(pin_tooltip).clicked() {
                        self.is_pinned = !self.is_pinned;
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            if self.is_pinned {
                                egui::WindowLevel::AlwaysOnTop
                            } else {
                                egui::WindowLevel::Normal
                            }
                        ));
                    }
                });
            });
            
            ui.add_space(8.0);

            // 设置面板
            if self.show_settings {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("API Key:");
                        let mut edit = egui::TextEdit::singleline(&mut self.api_key)
                            .desired_width(f32::INFINITY);
                        if !self.api_key_visible {
                            edit = edit.password(true);
                        }
                        // 当 api_key 内容变化时立即持久化
                        if ui.add(edit).changed() {
                            save_config(&AppConfig { api_key: self.api_key.clone() });
                        }

                        let eye_icon = if self.api_key_visible { "👁" } else { "" };
                        if ui.button(eye_icon).clicked() {
                            self.api_key_visible = !self.api_key_visible;
                        }
                    });
                    ui.label(
                        egui::RichText::new("配置自动保存，下次启动无需重新填写")
                            .small()
                            .color(egui::Color32::GRAY)
                    );
                });
                ui.add_space(8.0);
            }

            // ── 可滚动主体区域 ─────────────────────────────────────────────────
            // 注意：TextEdit 不直接嵌套在 ScrollArea 内——带滚动偏移的 TextEdit 会使
            // egui 上报给 Windows IME 的候选窗口坐标偏移，导致有文字时中文标点无法输入。
            // 正确做法：TextEdit 使用 desired_rows + 自身 auto-grow，外层 ScrollArea
            // 只负责在内容超出窗口时提供整体滚动。
            let scroll_height = ui.available_height() - 44.0;
            egui::ScrollArea::vertical()
                .id_salt("main_scroll")
                .max_height(scroll_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // ── 输入区标题行 ──
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("源文本").small().color(egui::Color32::GRAY));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("❌").on_hover_text("清空").clicked() {
                                self.input_text.clear();
                            }
                        });
                    });

                    // ── 输入框 ──
                    // 使用 changed() + ends_with('\n') 检测 Enter：
                    // • 用户按 Enter → TextEdit 向文本末尾追加 '\n'，changed() = true
                    // • IME 输入标点/汉字 → 追加的不是 '\n'，ends_with('\n') = false
                    // 两者可以可靠区分，无需关心 Enter 键事件
                    let input_resp = ui.add(
                        egui::TextEdit::multiline(&mut self.input_text)
                            .desired_width(ui.available_width())
                            .desired_rows(5)
                            .hint_text("输入要翻译的文本 (Enter 翻译, Shift+Enter 换行)")
                            .margin(egui::vec2(8.0, 8.0)),
                    );
                    if input_resp.changed()
                        && self.input_text.ends_with('\n')
                        && !ctx.input(|i| i.modifiers.shift)
                    {
                        self.input_text.pop();
                        self.trigger_translation(ctx);
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // ── 输出区标题行 ──
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("译文").small().color(egui::Color32::GRAY));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("📋").on_hover_text("复制").clicked() {
                                ctx.copy_text(self.output_text.clone());
                            }
                        });
                    });

                    // ── 输出框 ──
                    ui.add(
                        egui::TextEdit::multiline(&mut self.output_text.as_str())
                            .desired_width(ui.available_width())
                            .desired_rows(5)
                            .interactive(false)
                            .margin(egui::vec2(8.0, 8.0)),
                    );
                });

            ui.add_space(4.0);

            // 底部栏
            ui.horizontal(|ui| {
                let lang_text = format!("{} \u{2194} {}", self.source_lang.display(), self.target_lang.display());
                
                if ui.button(&lang_text).clicked() {
                    std::mem::swap(&mut self.source_lang, &mut self.target_lang);
                    std::mem::swap(&mut self.input_text, &mut self.output_text);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!self.is_translating, egui::Button::new("翻译")).clicked() {
                        self.trigger_translation(ctx);
                    }
                });
            });
        });
    }
}
