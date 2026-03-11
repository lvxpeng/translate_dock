use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use crate::translate::{translate_text, Language};
use tray_icon::{TrayIcon, TrayIconBuilder, Icon, TrayIconEvent, MouseButton, MouseButtonState};
use tray_icon::menu::{Menu, MenuItem, MenuEvent, MenuId};

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics,
    SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SystemParametersInfoW,
};
#[cfg(windows)]
use windows_sys::Win32::Foundation::RECT;

// ── 配置持久化 ──────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AppConfig {
    /// 加密后的 API Key（base64 编码）
    #[serde(default)]
    encrypted_api_key: String,
    /// 旧版明文字段，仅用于迁移兼容
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

// ── DPAPI 加解密 ──────────────────────────────────────────

#[cfg(windows)]
fn dpapi_encrypt(plaintext: &str) -> Option<String> {
    use base64::Engine;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB,
    };

    if plaintext.is_empty() {
        return Some(String::new());
    }

    let mut input_bytes = plaintext.as_bytes().to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input_bytes.len() as u32,
        pbData: input_bytes.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let result = unsafe {
        CryptProtectData(
            &input_blob,
            std::ptr::null(),    // description
            std::ptr::null(),    // optional entropy
            std::ptr::null(),    // reserved
            std::ptr::null(),    // prompt struct
            0,                   // flags
            &mut output_blob,
        )
    };

    if result == 0 {
        return None;
    }

    let encrypted = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };

    // 释放系统分配的内存
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(output_blob.pbData as *mut _);
    }

    Some(base64::engine::general_purpose::STANDARD.encode(&encrypted))
}

#[cfg(windows)]
fn dpapi_decrypt(encrypted_b64: &str) -> Option<String> {
    use base64::Engine;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    if encrypted_b64.is_empty() {
        return Some(String::new());
    }

    let mut encrypted = base64::engine::general_purpose::STANDARD
        .decode(encrypted_b64)
        .ok()?;

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let result = unsafe {
        CryptUnprotectData(
            &input_blob,
            std::ptr::null_mut(), // description
            std::ptr::null(),     // optional entropy
            std::ptr::null(),     // reserved
            std::ptr::null(),     // prompt struct
            0,                    // flags
            &mut output_blob,
        )
    };

    if result == 0 {
        return None;
    }

    let decrypted = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };

    unsafe {
        windows_sys::Win32::Foundation::LocalFree(output_blob.pbData as *mut _);
    }

    String::from_utf8(decrypted).ok()
}

#[cfg(not(windows))]
fn dpapi_encrypt(plaintext: &str) -> Option<String> {
    Some(plaintext.to_string())
}

#[cfg(not(windows))]
fn dpapi_decrypt(encrypted: &str) -> Option<String> {
    Some(encrypted.to_string())
}

fn load_config() -> (AppConfig, String) {
    let path = config_path();
    let config: AppConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // 优先从加密字段解密
    if !config.encrypted_api_key.is_empty() {
        let key = dpapi_decrypt(&config.encrypted_api_key).unwrap_or_default();
        return (config, key);
    }

    // 兼容旧版明文字段：自动迁移
    if !config.api_key.is_empty() {
        let key = config.api_key.clone();
        // 立即迁移为加密存储
        save_config_key(&key);
        return (config, key);
    }

    (config, String::new())
}

fn save_config_key(api_key: &str) {
    let encrypted = dpapi_encrypt(api_key).unwrap_or_default();
    let config = AppConfig {
        encrypted_api_key: encrypted,
        api_key: String::new(), // 清空明文字段
    };
    let path = config_path();
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = std::fs::write(path, json);
    }
}

// ── 获取屏幕工作区大小（排除任务栏） ───────────────────

#[cfg(windows)]
fn get_work_area() -> (f32, f32, f32, f32) {
    // 返回 (x, y, width, height) 工作区域（排除任务栏）
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let success = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut rect as *mut RECT as *mut _,
            0,
        )
    };
    if success != 0 {
        (
            rect.left as f32,
            rect.top as f32,
            (rect.right - rect.left) as f32,
            (rect.bottom - rect.top) as f32,
        )
    } else {
        // 回退：使用全屏大小
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
        (0.0, 0.0, w, h)
    }
}

#[cfg(not(windows))]
fn get_work_area() -> (f32, f32, f32, f32) {
    (0.0, 0.0, 1920.0, 1040.0)
}

/// 计算窗口初始位置：右下角，紧贴任务栏上边缘，右侧留一小段距离
pub fn calculate_initial_position(window_width: f32, window_height: f32) -> egui::Pos2 {
    let (wa_x, wa_y, wa_w, wa_h) = get_work_area();

    // outer_margin 是 egui Frame 的外边距，窗口实际内容会有这个偏移
    let margin = 16.0;
    let right_gap = 12.0; // 右侧留出的小距离

    let x = wa_x + wa_w - window_width - right_gap;
    let y = wa_y + wa_h - window_height - margin;

    egui::pos2(x, y)
}

// ── Win32 窗口操作 ─────────────────────────────────────────

// Win32 API 辅助代码仅保留用于工作区计算，窗口隐藏/显示使用 egui::ViewportCommand


// ── 应用事件 ───────────────────────────────────────────────

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
    /// 逻辑上是否「可见」
    show_window: bool,
    is_pinned: bool,
    /// 固定的窗口位置（每次显示都恢复到此位置）
    fixed_pos: egui::Pos2,

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
    
    // 使用 include_bytes! 嵌入微软雅黑字体
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
        let (_config, api_key) = load_config();

        // 计算固定位置
        let window_width = 432.0 + 32.0;  // inner_size + outer_margin * 2
        let window_height = 332.0 + 32.0;
        let fixed_pos = calculate_initial_position(window_width, window_height);

        Self {
            input_text: String::new(),
            output_text: String::new(),
            source_lang: Language::Chinese,
            target_lang: Language::English,
            is_translating: false,
            api_key,
            translation_rx: Some(rx),
            translation_tx: tx,
            show_settings: false,
            api_key_visible: false,
            _tray_icon: tray_icon,
            show_window: true,
            is_pinned: true,
            fixed_pos,
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

    fn do_hide(&mut self, ctx: &egui::Context) {
        self.show_window = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    fn do_show(&mut self, ctx: &egui::Context) {
        self.show_window = true;
        // 先恢复到固定位置
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(self.fixed_pos));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for TranslateApp {
    // 告诉 eframe 窗口背景是透明的
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    // 应用退出时保存配置（兜底，正常情况下 changed() 已实时保存）
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_config_key(&self.api_key);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 同步窗口状态：如果在原生系统（如点击任务栏）激活了窗口，更新内部状态
        let gained_focus = ctx.input(|i| i.events.iter().any(|e| {
            matches!(e, egui::Event::WindowFocused(true))
        }));
        if gained_focus {
            self.show_window = true;
        }

        // 处理失去焦点：非固定时隐藏窗口
        if !self.is_pinned && self.show_window {
            let lost_focus = ctx.input(|i| i.events.iter().any(|e| {
                matches!(e, egui::Event::WindowFocused(false))
            }));
            if lost_focus {
                self.do_hide(ctx);
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
                            // 当前可见 → 隐藏
                            self.do_hide(ctx);
                        } else {
                            // 当前隐藏 → 显示
                            self.do_show(ctx);
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
                        // 当 api_key 内容变化时立即持久化（加密存储）
                        if ui.add(edit).changed() {
                            save_config_key(&self.api_key);
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
