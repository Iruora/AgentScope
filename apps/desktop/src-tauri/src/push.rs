//! Phone push: the same alerts as desktop notifications, sent through a
//! service the user picks. Off by default; this and the update check are the
//! only things AgentScope ever sends off the machine.

use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

const TIMEOUT: Duration = Duration::from_secs(10);
const NTFY_DEFAULT: &str = "https://ntfy.sh";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PushService {
    #[default]
    Off,
    Ntfy,
    Pushover,
    Discord,
    Slack,
    Telegram,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PushSettings {
    pub service: PushService,
    /// ntfy topic or topic URL, Pushover user key, webhook URL, or Telegram chat id.
    pub target: String,
    /// Pushover app token, Telegram bot token, or an optional ntfy access token.
    pub token: String,
    /// Include the agent's name and the command. Off sends only that an agent is waiting.
    pub include_details: bool,
    /// Also push when an agent waits for input, not only for approvals.
    pub include_input: bool,
}

impl PushSettings {
    pub fn wants(&self, approval: bool) -> bool {
        self.service != PushService::Off && (approval || self.include_input)
    }

    /// Why these settings cannot send, if they cannot.
    pub fn validate(&self) -> Result<(), String> {
        let target = self.target.trim();
        let token = self.token.trim();
        let https = |what: &str| {
            if target.starts_with("https://") {
                Ok(())
            } else {
                Err(format!(
                    "Paste the {what} webhook URL, starting with https://"
                ))
            }
        };
        match self.service {
            PushService::Off => Ok(()),
            PushService::Ntfy if target.is_empty() => Err("Enter an ntfy topic".into()),
            PushService::Ntfy if target.contains("://") && !target.starts_with("https://") => {
                Err("The ntfy server must use https://".into())
            }
            PushService::Ntfy => Ok(()),
            PushService::Pushover if target.is_empty() || token.is_empty() => {
                Err("Pushover needs your user key and an app token".into())
            }
            PushService::Pushover => Ok(()),
            PushService::Discord => https("Discord"),
            PushService::Slack => https("Slack"),
            PushService::Telegram if target.is_empty() || token.is_empty() => {
                Err("Telegram needs a bot token and your chat id".into())
            }
            PushService::Telegram => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushMessage {
    pub title: String,
    pub body: String,
    pub approval: bool,
}

impl PushMessage {
    /// What to send for an alert, leaving out names and commands unless asked.
    pub fn new(settings: &PushSettings, title: &str, body: &str, approval: bool) -> Self {
        if settings.include_details {
            return Self {
                title: title.to_owned(),
                body: body.to_owned(),
                approval,
            };
        }
        Self {
            title: "AgentScope".into(),
            body: if approval {
                "An agent needs your approval".into()
            } else {
                "An agent is waiting for your input".into()
            },
            approval,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Body {
    Json(Value),
    Form(Vec<(&'static str, String)>),
}

#[derive(Debug, PartialEq)]
struct Request {
    url: String,
    bearer: Option<String>,
    body: Body,
}

fn ntfy_topic(target: &str) -> (String, String) {
    let target = target.trim().trim_end_matches('/');
    match target.rsplit_once('/') {
        Some((base, topic)) if target.contains("://") => (base.to_owned(), topic.to_owned()),
        _ => (NTFY_DEFAULT.to_owned(), target.to_owned()),
    }
}

fn request(settings: &PushSettings, msg: &PushMessage) -> Option<Request> {
    let target = settings.target.trim().to_owned();
    let token = settings.token.trim().to_owned();
    let text = format!("{}\n{}", msg.title, msg.body);
    Some(match settings.service {
        PushService::Off => return None,
        PushService::Ntfy => {
            let (base, topic) = ntfy_topic(&target);
            Request {
                url: base,
                bearer: (!token.is_empty()).then_some(token),
                body: Body::Json(json!({
                    "topic": topic,
                    "title": msg.title,
                    "message": msg.body,
                    "priority": if msg.approval { 4 } else { 3 },
                    "tags": ["robot"],
                })),
            }
        }
        PushService::Pushover => Request {
            url: "https://api.pushover.net/1/messages.json".into(),
            bearer: None,
            body: Body::Form(vec![
                ("token", token),
                ("user", target),
                ("title", msg.title.clone()),
                ("message", msg.body.clone()),
            ]),
        },
        PushService::Discord => Request {
            url: target,
            bearer: None,
            body: Body::Json(json!({ "username": "AgentScope", "content": text })),
        },
        PushService::Slack => Request {
            url: target,
            bearer: None,
            body: Body::Json(json!({ "text": text })),
        },
        PushService::Telegram => Request {
            url: format!("https://api.telegram.org/bot{token}/sendMessage"),
            bearer: None,
            body: Body::Json(json!({ "chat_id": target, "text": text })),
        },
    })
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        // Fails only when another thread installed one first, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("AgentScope/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

/// Sends one message and reports why it failed, without echoing the token.
pub async fn send(settings: &PushSettings, msg: &PushMessage) -> Result<(), String> {
    settings.validate()?;
    let Some(req) = request(settings, msg) else {
        return Ok(());
    };
    let mut builder = CLIENT.post(&req.url);
    if let Some(bearer) = &req.bearer {
        builder = builder.bearer_auth(bearer);
    }
    builder = match &req.body {
        Body::Json(v) => builder.json(v),
        Body::Form(f) => builder.form(f),
    };
    let response = builder
        .send()
        .await
        .map_err(|e| format!("Could not reach the push service: {}", e.without_url()))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!(
            "The push service answered {status}. Check the settings."
        ))
    }
}

/// Sends in the background, logging failures.
pub fn spawn_send(settings: PushSettings, msg: PushMessage) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = send(&settings, &msg).await {
            tracing::warn!(service = ?settings.service, %err, "push failed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(service: PushService, target: &str, token: &str) -> PushSettings {
        PushSettings {
            service,
            target: target.into(),
            token: token.into(),
            ..PushSettings::default()
        }
    }

    #[test]
    fn hides_details_unless_asked() {
        let mut s = with(PushService::Ntfy, "t", "");
        let msg = PushMessage::new(&s, "shop", "Wants to run Bash: rm -rf x", true);
        assert_eq!(msg.title, "AgentScope");
        assert!(!msg.body.contains("rm"));
        s.include_details = true;
        let msg = PushMessage::new(&s, "shop", "Wants to run Bash: rm -rf x", true);
        assert_eq!(
            (msg.title.as_str(), msg.body.as_str()),
            ("shop", "Wants to run Bash: rm -rf x")
        );
    }

    #[test]
    fn only_approvals_unless_input_too() {
        let mut s = with(PushService::Ntfy, "t", "");
        assert!(s.wants(true) && !s.wants(false));
        s.include_input = true;
        assert!(s.wants(false));
        assert!(!PushSettings::default().wants(true));
    }

    #[test]
    fn builds_each_service_request() {
        let msg = PushMessage {
            title: "T".into(),
            body: "B".into(),
            approval: true,
        };
        let r = request(&with(PushService::Ntfy, "my-topic", ""), &msg).unwrap();
        assert_eq!(r.url, NTFY_DEFAULT);
        assert!(matches!(&r.body, Body::Json(v) if v["topic"] == "my-topic" && v["priority"] == 4));
        let r = request(
            &with(PushService::Ntfy, "https://ntfy.example.com/team/", "tk"),
            &msg,
        )
        .unwrap();
        assert_eq!(r.url, "https://ntfy.example.com");
        assert_eq!(r.bearer.as_deref(), Some("tk"));
        assert!(matches!(&r.body, Body::Json(v) if v["topic"] == "team"));

        let r = request(&with(PushService::Pushover, "user", "app"), &msg).unwrap();
        assert_eq!(
            r.body,
            Body::Form(vec![
                ("token", "app".into()),
                ("user", "user".into()),
                ("title", "T".into()),
                ("message", "B".into())
            ])
        );

        let r = request(&with(PushService::Telegram, "42", "bot:abc"), &msg).unwrap();
        assert_eq!(r.url, "https://api.telegram.org/botbot:abc/sendMessage");
        assert!(matches!(&r.body, Body::Json(v) if v["chat_id"] == "42" && v["text"] == "T\nB"));

        let r = request(
            &with(PushService::Slack, "https://hooks.slack.com/x", ""),
            &msg,
        )
        .unwrap();
        assert!(matches!(&r.body, Body::Json(v) if v["text"] == "T\nB"));
        assert!(request(&PushSettings::default(), &msg).is_none());
    }

    #[test]
    fn validates_before_sending() {
        assert!(with(PushService::Off, "", "").validate().is_ok());
        assert!(with(PushService::Ntfy, "", "").validate().is_err());
        assert!(with(PushService::Ntfy, "http://ntfy.local/t", "")
            .validate()
            .is_err());
        assert!(with(PushService::Discord, "discord.com/api/webhooks/1", "")
            .validate()
            .is_err());
        assert!(with(
            PushService::Discord,
            "https://discord.com/api/webhooks/1",
            ""
        )
        .validate()
        .is_ok());
        assert!(with(PushService::Telegram, "42", "").validate().is_err());
        assert!(with(PushService::Pushover, "u", "a").validate().is_ok());
    }
}
