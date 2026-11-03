//! Email over SMTP with lettre: STARTTLS (required) or TLS from the start, over rustls (ring)
//! with Mozilla's roots, and the user's credentials when it has some.

use std::time::Duration;

use lettre::{
    AsyncSmtpTransport, AsyncTransport as _, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::{
        authentication::Credentials,
        client::{Certificate, Tls, TlsParameters},
    },
};
use rustls::pki_types::CertificateDer;
use uniwrt_model::notify::{EmailSpec, SmtpSecurity};

use crate::{Secrets, SendError, render::Email};

pub(crate) async fn send(
    spec: &EmailSpec,
    secrets: &Secrets,
    email: &Email,
    extra_roots: &[CertificateDer<'static>],
    timeout: Duration,
) -> Result<(), SendError> {
    let invalid = |what: &str| SendError::Invalid(format!("{what} is not an email address"));
    let from: Mailbox = spec.from.parse().map_err(|_| invalid(&spec.from))?;
    let mut builder = lettre::Message::builder()
        .from(from)
        .subject(&email.subject);
    for to in &spec.to {
        builder = builder.to(to.parse().map_err(|_| invalid(to))?);
    }
    let message = builder
        .multipart(MultiPart::alternative_plain_html(
            email.plain.clone(),
            email.html.clone(),
        ))
        .map_err(|err| SendError::Invalid(err.to_string()))?;

    let mut tls = TlsParameters::builder(spec.host.clone());
    for cert in extra_roots {
        let cert = Certificate::from_der(cert.to_vec()).map_err(|err| tls_error(&err))?;
        tls = tls.add_root_certificate(cert);
    }
    let tls = tls.build_rustls().map_err(|err| tls_error(&err))?;
    let tls = match spec.security {
        SmtpSecurity::Starttls => Tls::Required(tls),
        SmtpSecurity::Tls => Tls::Wrapper(tls),
    };
    let mut transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(spec.host.as_str())
        .port(spec.port)
        .tls(tls)
        .timeout(Some(timeout));
    if let Some(user) = &spec.username {
        let password = secrets.password.clone().unwrap_or_default();
        transport = transport.credentials(Credentials::new(user.clone(), password));
    }
    let transport = transport.build();
    match tokio::time::timeout(timeout * 2, transport.send(message)).await {
        Err(_) => Err(SendError::Timeout((timeout * 2).as_secs())),
        Ok(Err(err)) => Err(SendError::Smtp {
            transient: !err.is_permanent(),
            message: crate::http::chain(&err),
        }),
        Ok(Ok(_)) => Ok(()),
    }
}

fn tls_error(err: &lettre::transport::smtp::Error) -> SendError {
    SendError::Invalid(format!("TLS: {}", crate::http::chain(err)))
}
