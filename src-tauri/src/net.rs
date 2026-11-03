//! Network side of the translator.

use notch_core::settings::Settings;
use notch_core::translate::{self, Lang, Provider};
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct Translation {
    pub text: String,
    pub from: Lang,
    pub to: Lang,
    pub provider: Provider,
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(6))
            .user_agent(concat!("Notch/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client")
    })
}

async fn read_body(resp: reqwest::Response) -> Result<String, String> {
    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("сеть: {e}"))?;
    if status.is_success() || body.trim_start().starts_with('{') {
        Ok(body)
    } else {
        Err(format!("сервер ответил {status}"))
    }
}

pub async fn translate(settings: &Settings, text: &str) -> Result<Translation, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("нечего переводить".into());
    }
    let (from, to) = translate::direction(text);
    let net = |e: reqwest::Error| {
        if e.is_timeout() { "сервер не ответил вовремя".to_string() } else { format!("сеть: {e}") }
    };
    let result = match settings.provider {
        Provider::Google => {
            // Very long texts do not fit into a GET URL; the endpoint also accepts POST.
            let resp = if text.len() > 1500 {
                let url = translate::google_url("", from, to);
                let base = url.trim_end_matches("&q=");
                client().post(base).form(&[("q", text)]).send().await.map_err(net)?
            } else {
                client().get(translate::google_url(text, from, to)).send().await.map_err(net)?
            };
            translate::parse_google(&read_body(resp).await?)?
        }
        Provider::Deepl => {
            let key = settings.deepl_key.trim();
            if key.is_empty() {
                return Err("укажите ключ DeepL в настройках".into());
            }
            let resp = client()
                .post(translate::deepl_url(key))
                .header("Authorization", format!("DeepL-Auth-Key {key}"))
                .json(&translate::deepl_body(text, from, to))
                .send()
                .await
                .map_err(net)?;
            if resp.status().as_u16() == 403 {
                return Err("DeepL отклонил ключ".into());
            }
            translate::parse_deepl(&read_body(resp).await?)?
        }
        Provider::Llm => {
            if settings.llm_url.is_empty() {
                return Err("укажите адрес локальной модели в настройках".into());
            }
            let mut req = client()
                .post(&settings.llm_url)
                .timeout(Duration::from_secs(90))
                .json(&translate::llm_body(text, from, to, &settings.llm_model));
            if !settings.llm_key.trim().is_empty() {
                req = req.bearer_auth(settings.llm_key.trim());
            }
            let resp = req.send().await.map_err(|e| {
                if e.is_connect() { "локальная модель недоступна — запущен ли сервер?".to_string() } else { net(e) }
            })?;
            translate::parse_llm(&read_body(resp).await?)?
        }
    };
    Ok(Translation { text: result, from, to, provider: settings.provider })
}
