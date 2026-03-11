# Translate Dock 翻译工具

一个轻量级的桌面翻译工具，使用 Rust 和 egui 构建，整合阿里云的 Qwen MT 翻译 API，提供快速、便捷的文本翻译功能。占用小，可直接挂载在系统后台，主要解决突然需要翻译一些字词又懒得开一个浏览器标签。


### 主要特性

- 🚀 **高性能**：用 Rust 编写，编译优化充分，启动快速
- 🎯 **简洁界面**：基于 egui 的现代化 UI，无痛集成本地应用
- 📌 **系统托盘集成**：后台运行，随时调用
- 🔒 **安全轻量**：零外部依赖的翻译引擎
- ⚡ **异步翻译**：基于 Tokio 的异步处理，不阻塞 UI

## 🛠️ 技术栈

| 技术 | 说明 |
|------|------|
| **语言** | Rust |
| **UI 框架** | egui 0.33.3 (immediate-mode GUI) |
| **HTTP 客户端** | reqwest 0.12 |
| **异步运行时** | Tokio 1.37 |
| **翻译 API** | ali Qwen MT Plus API |
| **系统集成** | tray-icon, windows-sys |
| **序列化** | serde, serde_json |
| **日志** | log, env_logger |


### 核心模块说明

#### `main.rs`
- 应用程序主入口
- 初始化 tokio 异步运行时
- 配置 egui 窗口（位置、大小、透明度、图标）
- 窗口自动定位到屏幕右下角

#### `app.rs`
- 实现 `TranslateApp` 结构体
- 管理 UI 状态（输入文本、翻译结果、语言选择）
- 支持系统托盘集成
- 处理用户交互和事件

#### `translate.rs`
- 定义 `Language` 枚举
- `translate_text()` 异步函数实现翻译业务逻辑
- HTTP 请求构建和响应解析
- API 错误处理



## 🔌 支持的语言

默认中英文



## 📦 依赖说明

### 核心依赖
- **eframe/egui**：跨平台 GUI 框架
- **reqwest**：异步 HTTP 客户端
- **tokio**：异步运行时
- **serde**：序列化/反序列化
