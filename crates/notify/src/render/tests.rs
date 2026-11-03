//! Each format's body for one alert, a digest and a test; templates and their errors; email.

use serde_json::{Value, json};
use uniwrt_model::{
    alerts::AlertResolution,
    notify::{EmailSpec, NotifierSpec, NtfySpec, SmtpSecurity, WebhookFormat, WebhookSpec},
};

use super::{Rendered, render, template};
use crate::{Header, Item, Kind, Message, Names, Secrets, validate::sample};

const LINK: &str = "https://uniwrt.example";

fn webhook(format: WebhookFormat, template: Option<&str>, message: &Message) -> Value {
    let spec = NotifierSpec::Webhook(WebhookSpec {
        format,
        template: template.map(str::to_owned),
    });
    let mut secrets = Secrets::default();
    secrets.url = Some("https://hooks.example/abc".to_owned());
    secrets.headers = vec![Header {
        name: "Authorization".to_owned(),
        value: "Bearer s3cret".to_owned(),
    }];
    let Ok(Rendered::Http(request)) = render(&spec, &secrets, message) else {
        panic!("not an HTTP request");
    };
    assert_eq!(request.url.as_str(), "https://hooks.example/abc");
    assert_eq!(request.headers[0].0, "Authorization");
    serde_json::from_slice(&request.body).unwrap()
}

fn digest() -> Message {
    let one = sample().items.remove(0);
    let mut alert = one.alert.clone();
    alert.id = uniwrt_core::AlertId::new(2);
    alert.state.level = uniwrt_model::events::EventLevel::Critical;
    alert.state.resolved_at = Some(alert.state.started_at);
    alert.state.resolution = Some(AlertResolution::Cleared);
    let two = Item::new(Kind::Resolved, alert, &Names::default(), Some(LINK));
    Message::new(vec![one, two], Some(LINK), sample().at)
}

#[test]
fn one_alert_in_words() {
    let message = sample();
    assert_eq!(message.title, "CPU high · Node 1");
    assert_eq!(
        message.text,
        "CPU 93 %, above 90 % · worst 97 %\nWarning · firing for 0s, since 2026-11-02 14:02 UTC"
    );
    assert_eq!(
        message.link.as_deref(),
        Some("https://uniwrt.example/insights/alerts?alert=1")
    );
    let digest = digest();
    assert_eq!(digest.kind, Kind::Digest);
    assert_eq!(digest.title, "2 alerts: 1 firing, 1 resolved");
    assert_eq!(
        digest.text,
        "• CPU high · Node 1 (Warning)\n• Resolved: CPU high · Node 1 (Critical)"
    );
    assert_eq!(
        digest.link.as_deref(),
        Some("https://uniwrt.example/insights/alerts")
    );
}

#[test]
fn generic_json_lists_the_alerts() {
    let body = webhook(WebhookFormat::Generic, None, &digest());
    assert_eq!(body["source"], "uniwrt");
    assert_eq!(body["event"], "digest");
    assert_eq!(body["level"], "critical");
    assert_eq!(body["alerts"][0]["event"], "opened");
    assert_eq!(body["alerts"][0]["entity"], "node:1");
    assert_eq!(body["alerts"][0]["node"], 1);
    assert_eq!(body["alerts"][1]["resolution"], "cleared");
    assert_eq!(
        body["alerts"][1]["url"],
        "https://uniwrt.example/insights/alerts?alert=2"
    );
    let ha = webhook(WebhookFormat::HomeAssistant, None, &digest());
    assert_eq!(ha, body);
}

#[test]
fn services_formats() {
    let message = sample();
    let discord = webhook(WebhookFormat::Discord, None, &message);
    assert_eq!(discord["allowed_mentions"], json!({ "parse": [] }));
    assert_eq!(discord["embeds"][0]["title"], "CPU high · Node 1");
    assert_eq!(discord["embeds"][0]["color"], 0xE3_74_00);
    assert_eq!(
        discord["embeds"][0]["timestamp"],
        "2026-11-02T14:02:00.000Z"
    );
    assert_eq!(discord["embeds"][0]["footer"]["text"], "uniwrt · Warning");

    let slack = webhook(WebhookFormat::Slack, None, &message);
    assert_eq!(slack["text"], "CPU high · Node 1");
    assert_eq!(slack["attachments"][0]["color"], "#e37400");
    let section = slack["attachments"][0]["blocks"][0]["text"]["text"]
        .as_str()
        .unwrap();
    assert!(section.starts_with("*<https://uniwrt.example/insights/alerts?alert=1|CPU high"));

    let mut tricky = sample();
    tricky.title = "a <b> & c".to_owned();
    tricky.link = None;
    let slack = webhook(WebhookFormat::Slack, None, &tricky);
    let section = slack["attachments"][0]["blocks"][0]["text"]["text"]
        .as_str()
        .unwrap();
    assert!(section.starts_with("*a &lt;b&gt; &amp; c*\n"));

    let teams = webhook(WebhookFormat::Teams, None, &message);
    let card = &teams["attachments"][0]["content"];
    assert_eq!(
        teams["attachments"][0]["contentType"],
        "application/vnd.microsoft.card.adaptive"
    );
    assert_eq!(card["type"], "AdaptiveCard");
    assert_eq!(card["body"][0]["color"], "Warning");
    assert_eq!(card["body"].as_array().unwrap().len(), 4);
    assert_eq!(
        card["actions"][0]["url"],
        "https://uniwrt.example/insights/alerts?alert=1"
    );
    let resolved = webhook(
        WebhookFormat::Teams,
        None,
        &Message::new(vec![digest().items.remove(1)], None, sample().at),
    );
    assert_eq!(
        resolved["attachments"][0]["content"]["body"][0]["color"],
        "Good"
    );
}

#[test]
fn ntfy_by_level_with_auth() {
    let spec = |username: Option<&str>| {
        NotifierSpec::Ntfy(NtfySpec {
            server: "https://ntfy.example/".to_owned(),
            topic: "home".to_owned(),
            username: username.map(str::to_owned),
        })
    };
    let mut secrets = Secrets::default();
    secrets.token = Some("tk_abc".to_owned());
    let Ok(Rendered::Http(request)) = render(&spec(None), &secrets, &sample()) else {
        panic!("not HTTP");
    };
    assert_eq!(request.url.as_str(), "https://ntfy.example");
    assert_eq!(request.headers[0].1.as_str(), "Bearer tk_abc");
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["topic"], "home");
    assert_eq!(body["priority"], 4);
    assert_eq!(body["tags"], json!(["warning"]));
    assert_eq!(
        body["click"],
        "https://uniwrt.example/insights/alerts?alert=1"
    );

    let mut secrets = Secrets::default();
    secrets.password = Some("pw".to_owned());
    let Ok(Rendered::Http(request)) = render(
        &spec(Some("me")),
        &secrets,
        &Message::test(None, sample().at),
    ) else {
        panic!("not HTTP");
    };
    assert_eq!(request.headers[0].1.as_str(), "Basic bWU6cHc=");
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["priority"], 3);
    assert!(body.get("click").is_none());
}

#[test]
fn templates_fill_placeholders() {
    let template = r#"{"text": "{{ title }}: {{message}}", "node": {{node}}, "id": {{alert_id}},
                      "level": "{{level}}", "event": "{{event}}", "n": {{alerts}}}"#;
    let body = webhook(WebhookFormat::Custom, Some(template), &sample());
    assert_eq!(body["node"], 1);
    assert_eq!(body["id"], 1);
    assert_eq!(body["level"], "warning");
    assert_eq!(body["event"], "opened");
    assert_eq!(body["n"], 1);
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .contains("CPU 93 %, above 90 %")
    );
    // A test has no alert: number placeholders are null.
    let test = template::render(template, &Message::test(None, sample().at)).unwrap();
    let test: Value = serde_json::from_str(&test).unwrap();
    assert_eq!(test["node"], Value::Null);
    assert!(template::check(template, &sample()).is_ok());

    let fails = |template: &str| template::check(template, &sample()).unwrap_err();
    assert_eq!(fails(""), "is empty");
    assert_eq!(fails(r#"{"a": "{{nope}}"}"#), "has no placeholder {{nope}}");
    assert_eq!(fails(r#"{"a": "{{title"}"#), "has a {{ without its }}");
    // Unquoted text does not make JSON; a quoted number is text.
    assert!(fails(r#"{"a": {{title}}}"#).starts_with("does not make JSON"));
    assert!(template::check(r#"{"a": "{{node}}"}"#, &sample()).is_ok());
}

#[test]
fn emails_escape_html() {
    let mut message = sample();
    message.title = "<script>".to_owned();
    let spec = NotifierSpec::Email(EmailSpec {
        host: "smtp.example".to_owned(),
        port: 587,
        security: SmtpSecurity::Starttls,
        username: None,
        from: "uniwrt <uniwrt@example.com>".to_owned(),
        to: vec!["me@example.com".to_owned()],
    });
    let Ok(Rendered::Email(email)) = render(&spec, &Secrets::default(), &message) else {
        panic!("not an email");
    };
    assert_eq!(email.subject, "[uniwrt] <script>");
    assert!(email.html.contains("&lt;script&gt;"));
    assert!(!email.html.contains("<script>"));
    assert!(
        email
            .html
            .contains("href=\"https://uniwrt.example/insights/alerts?alert=1\"")
    );
    assert!(email.plain.contains("CPU 93 %, above 90 %"));
    assert!(email.plain.ends_with("Sent by uniwrt\n"));
}
