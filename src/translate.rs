use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum Language {
    Auto,
    English,
    Chinese,
    Japanese,
}

impl Language {
    /// 返回阿里云通义翻译 API 要求的语言全称
    pub fn full_name(&self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::English => "English",
            Language::Chinese => "Chinese",
            Language::Japanese => "Japanese",
        }
    }

    pub fn display(&self) -> &'static str {
        match self {
            Language::Auto => "自动",
            Language::English => "英语",
            Language::Chinese => "中文",
            Language::Japanese => "日语",
        }
    }
}

// ── 请求结构 ────────────────────────────────────────────────

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct TranslationOptions {
    source_lang: String,
    target_lang: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: &'static str,
    messages: Vec<Message>,
    translation_options: TranslationOptions,
}

// ── 响应结构 ────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

// ── 主函数 ─────────────────────────────────────────────────

pub async fn translate_text(
    text: &str,
    source: Language,
    target: Language,
    api_key: &str,
) -> Result<String, String> {
    if api_key.is_empty() {
        // 未配置 API Key 时返回模拟结果，方便界面调试
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Ok(format!("[未配置 API Key，模拟翻译] {}", text));
    }

    let client = reqwest::Client::new();

    let body = ChatRequest {
        model: "qwen-mt-plus",
        messages: vec![Message {
            role: "user",
            content: text.to_string(),
        }],
        translation_options: TranslationOptions {
            source_lang: source.full_name().to_string(),
            target_lang: target.full_name().to_string(),
        },
    };

    let res = client
        .post("https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("网络错误: {}", e))?;

    if res.status().is_success() {
        let chat: ChatResponse = res
            .json()
            .await
            .map_err(|e| format!("响应解析失败: {}", e))?;

        chat.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| "API 返回了空的 choices".to_string())
    } else {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        Err(format!("API 错误 {}: {}", status, body))
    }
}

