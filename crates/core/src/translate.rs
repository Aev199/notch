//! Translation helpers: RU/EN direction detection, request building and
//! response parsing for the supported providers. Networking itself lives in
//! the Tauri shell; this module only deals with data.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    fn deepl_target(self) -> &'static str {
        match self {
            Lang::Ru => "RU",
            Lang::En => "EN-US",
        }
    }

    fn deepl_source(self) -> &'static str {
        match self {
            Lang::Ru => "RU",
            Lang::En => "EN",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Lang::Ru => "Russian",
            Lang::En => "English",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Built-in offline engine (Bergamot / Firefox Translations models, runs
    /// in the webview as WebAssembly; no network at all).
    #[default]
    Offline,
    /// Free public web endpoint of Google Translate (no key).
    Google,
    /// DeepL API (free or pro key).
    Deepl,
    /// Any OpenAI-compatible chat endpoint (Ollama, LM Studio, llama.cpp...).
    Llm,
}

/// Source and target language of a fragment: anything containing Cyrillic is
/// treated as Russian and translated to English, everything else goes to Russian.
pub fn direction(text: &str) -> (Lang, Lang) {
    let cyrillic = text.chars().filter(|c| matches!(c, '\u{0400}'..='\u{04FF}')).count();
    let latin = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    // A Russian sentence with a couple of English terms is still Russian; an
    // English text with a single Cyrillic word is still English.
    if cyrillic > 0 && cyrillic * 3 >= latin {
        (Lang::Ru, Lang::En)
    } else {
        (Lang::En, Lang::Ru)
    }
}

fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Full GET URL of the free Google endpoint.
pub fn google_url(text: &str, from: Lang, to: Lang) -> String {
    format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
        from.code(),
        to.code(),
        encode_query(text)
    )
}

/// The endpoint answers with nested arrays: `[[["перевод","source",...],...],...]`.
pub fn parse_google(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("непонятный ответ Google: {e}"))?;
    let segments = value.get(0).and_then(Value::as_array).ok_or("в ответе Google нет перевода")?;
    let text: String = segments.iter().filter_map(|seg| seg.get(0).and_then(Value::as_str)).collect();
    if text.is_empty() {
        return Err("Google вернул пустой перевод".into());
    }
    Ok(text)
}

/// DeepL host depends on the key type: free keys end with `:fx`.
pub fn deepl_url(key: &str) -> &'static str {
    if key.trim().ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    }
}

pub fn deepl_body(text: &str, from: Lang, to: Lang) -> Value {
    json!({ "text": [text], "source_lang": from.deepl_source(), "target_lang": to.deepl_target() })
}

pub fn parse_deepl(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("непонятный ответ DeepL: {e}"))?;
    if let Some(msg) = value.get("message").and_then(Value::as_str) {
        return Err(format!("DeepL: {msg}"));
    }
    value["translations"]
        .as_array()
        .map(|list| list.iter().filter_map(|t| t["text"].as_str()).collect::<Vec<_>>().join("\n"))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "в ответе DeepL нет перевода".into())
}

pub fn llm_body(text: &str, from: Lang, to: Lang, model: &str) -> Value {
    json!({
        "model": model,
        "temperature": 0.2,
        "stream": false,
        "messages": [
            {
                "role": "system",
                "content": format!(
                    "You are a translation engine. Translate the user's text from {} to {}. \
                     Reply with the translation only: no quotes, no comments, keep formatting.",
                    from.name(), to.name()
                )
            },
            { "role": "user", "content": text }
        ]
    })
}

pub fn parse_llm(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("непонятный ответ модели: {e}"))?;
    if let Some(err) = value.get("error") {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("ошибка сервера");
        return Err(format!("модель: {msg}"));
    }
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "модель вернула пустой ответ".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_by_cyrillic() {
        assert_eq!(direction("Привет, мир"), (Lang::Ru, Lang::En));
        assert_eq!(direction("Hello world"), (Lang::En, Lang::Ru));
        assert_eq!(direction("Запусти cargo build ещё раз"), (Lang::Ru, Lang::En));
        assert_eq!(direction("The word дом means house in this long sentence"), (Lang::En, Lang::Ru));
        assert_eq!(direction("12345"), (Lang::En, Lang::Ru));
    }

    #[test]
    fn google_url_is_encoded() {
        let url = google_url("Да & нет?", Lang::Ru, Lang::En);
        assert!(url.ends_with("sl=ru&tl=en&dt=t&q=%D0%94%D0%B0%20%26%20%D0%BD%D0%B5%D1%82%3F"));
    }

    #[test]
    fn google_parsing() {
        let body = r#"[[["Hello, ","Привет, ",null,null,10],["world","мир",null,null,10]],null,"ru"]"#;
        assert_eq!(parse_google(body).unwrap(), "Hello, world");
        assert!(parse_google("[]").is_err());
        assert!(parse_google("<html>").is_err());
    }

    #[test]
    fn deepl_parts() {
        assert_eq!(deepl_url("abc:fx"), "https://api-free.deepl.com/v2/translate");
        assert_eq!(deepl_url("abc"), "https://api.deepl.com/v2/translate");
        assert_eq!(deepl_body("hi", Lang::En, Lang::Ru)["target_lang"], "RU");
        let ok = r#"{"translations":[{"detected_source_language":"EN","text":"привет"}]}"#;
        assert_eq!(parse_deepl(ok).unwrap(), "привет");
        assert_eq!(parse_deepl(r#"{"message":"Wrong key"}"#).unwrap_err(), "DeepL: Wrong key");
    }

    #[test]
    fn llm_parts() {
        let body = llm_body("hi", Lang::En, Lang::Ru, "qwen2.5");
        assert_eq!(body["model"], "qwen2.5");
        assert_eq!(body["messages"][1]["content"], "hi");
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":"  привет \n"}}]}"#;
        assert_eq!(parse_llm(ok).unwrap(), "привет");
        assert!(parse_llm(r#"{"error":{"message":"no model"}}"#).unwrap_err().contains("no model"));
    }
}
