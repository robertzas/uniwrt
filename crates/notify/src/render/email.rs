//! An email's subject and bodies: plain text, and HTML that reads well in mail clients (inline
//! styles, one table, no images).

use std::fmt::Write as _;

use super::{color, label};
use crate::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    pub subject: String,
    pub plain: String,
    pub html: String,
}

pub(super) fn render(message: &Message) -> Email {
    let mut plain = format!("{}\n\n{}\n", message.title, message.text);
    if let Some(link) = &message.link {
        let _ = write!(plain, "\n{link}\n");
    }
    plain.push_str("\n-- \nSent by uniwrt\n");

    let accent = format!("#{:06x}", color(message));
    let mut html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"></head>\
         <body style=\"margin:0;padding:24px;background:#f4f5f7;font-family:-apple-system,\
         'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#202124\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" \
         style=\"max-width:560px;margin:0 auto;background:#ffffff;border-radius:8px;\
         border-top:4px solid {accent}\"><tr><td style=\"padding:20px 24px\">\
         <div style=\"font-size:12px;font-weight:600;letter-spacing:.04em;\
         text-transform:uppercase;color:{accent}\">{}</div>\
         <h1 style=\"font-size:18px;line-height:1.35;margin:6px 0 12px\">{}</h1>",
        label(message),
        escape(&message.title)
    );
    for line in message.text.lines() {
        let _ = write!(
            html,
            "<p style=\"margin:0 0 6px;font-size:14px;line-height:1.5\">{}</p>",
            escape(line)
        );
    }
    if let Some(link) = &message.link {
        let _ = write!(
            html,
            "<p style=\"margin:16px 0 0\"><a href=\"{}\" style=\"display:inline-block;\
             padding:8px 14px;background:#1a73e8;color:#ffffff;border-radius:6px;\
             text-decoration:none;font-size:14px\">Open in uniwrt</a></p>",
            escape(link)
        );
    }
    html.push_str(
        "</td></tr></table><p style=\"text-align:center;font-size:12px;color:#5f6368\">\
         Sent by uniwrt</p></body></html>",
    );
    Email {
        subject: format!("[uniwrt] {}", message.title),
        plain,
        html,
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
