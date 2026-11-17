//! AWS IAM sign-in to Amazon RDS. A token signed with the user's AWS credentials takes the place
//! of the password. It is a SigV4 presigned URL for `rds-db:connect`, built as `aws-sdk-rds`'s
//! `auth_token` builds it, without pulling in the whole RDS client.

use std::time::{Duration, SystemTime};

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_credential_types::provider::ProvideCredentials;
use aws_credential_types::provider::error::CredentialsError;
use aws_sigv4::http_request::{
    self, SignableBody, SignableRequest, SignatureLocation, SigningSettings,
};
use aws_sigv4::sign::v4;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};
use aws_smithy_runtime_api::client::identity::Identity;

/// RDS accepts a token for at most 15 minutes. Every session signs a new one, so a long-open
/// connection never needs it again.
const LIFETIME: Duration = Duration::from_secs(900);
const SERVICE: &str = "rds-db";

/// The database endpoint the token is signed for. It must be the RDS host name itself, also
/// when the connection goes through an SSH tunnel.
pub(crate) struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
}

/// A token for `target`, signed with the credentials of `profile` (or the default chain).
pub(crate) async fn token(
    target: &Target<'_>,
    profile: Option<&str>,
    region: Option<&str>,
) -> Result<String, String> {
    // TLS through ring, as everywhere else in Quarry. Used for SSO and role credentials.
    let http = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
        .build_https();
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).http_client(http);
    if let Some(profile) = profile {
        loader = loader.profile_name(profile);
    }
    let sdk = loader.load().await;
    let region = region
        .map(str::to_owned)
        .or_else(|| region_from_host(target.host))
        .or_else(|| sdk.region().map(ToString::to_string))
        .ok_or_else(|| {
            "No AWS region for this connection. Set the region, or set one in the AWS profile."
                .to_owned()
        })?;
    let provider = sdk
        .credentials_provider()
        .ok_or_else(|| no_credentials(profile, NOT_FOUND))?;
    let credentials = provider
        .provide_credentials()
        .await
        .map_err(|err| no_credentials(profile, &cause(&err)))?;
    sign(target, &region, credentials, SystemTime::now())
}

const NOT_FOUND: &str = "none was found in the environment, ~/.aws or an SSO session";

/// Why credentials failed, in words: the provider's messages, without its debug output.
fn cause(err: &CredentialsError) -> String {
    match err {
        CredentialsError::CredentialsNotLoaded(_) => NOT_FOUND.to_owned(),
        CredentialsError::ProviderTimedOut(_) => "the credential provider timed out".to_owned(),
        _ => {
            let mut parts: Vec<String> = Vec::new();
            let mut next: Option<&dyn std::error::Error> = Some(err);
            while let Some(e) = next {
                let text = e.to_string();
                if !parts.contains(&text) {
                    parts.push(text);
                }
                next = e.source();
            }
            parts.join(": ")
        }
    }
}

fn no_credentials(profile: Option<&str>, cause: &str) -> String {
    let (name, flag) = match profile {
        Some(p) => (format!("profile “{p}”"), format!(" --profile {p}")),
        None => ("the default profile".to_owned(), String::new()),
    };
    format!(
        "AWS credentials for {name} could not be loaded: {cause}. Sign in with `aws sso login{flag}`, or check ~/.aws/config and ~/.aws/credentials."
    )
}

/// The token for `target` in `region`, signed at `time`.
fn sign(
    target: &Target<'_>,
    region: &str,
    credentials: Credentials,
    time: SystemTime,
) -> Result<String, String> {
    let failed =
        |err: &dyn std::fmt::Display| format!("The AWS IAM token could not be signed: {err}.");
    let identity: Identity = credentials.into();
    let mut settings = SigningSettings::default();
    settings.expires_in = Some(LIFETIME);
    settings.signature_location = SignatureLocation::QueryParams;
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(SERVICE)
        .time(time)
        .settings(settings)
        .build()
        .map_err(|e| failed(&e))?;
    // The user goes in unencoded, as the AWS SDKs do.
    let base = format!(
        "https://{}:{}/?Action=connect&DBUser={}",
        target.host, target.port, target.user
    );
    let request = SignableRequest::new("GET", &base, std::iter::empty(), SignableBody::empty())
        .map_err(|e| failed(&e))?;
    let (instructions, _) = http_request::sign(request, &params.into())
        .map_err(|e| failed(&e))?
        .into_parts();
    let mut url = url::Url::parse(&base).map_err(|e| failed(&e))?;
    for (name, value) in instructions.params() {
        url.query_pairs_mut().append_pair(name, value);
    }
    Ok(url.as_str().trim_start_matches("https://").to_owned())
}

/// The region in an RDS host name: `db.abc123.eu-west-1.rds.amazonaws.com` → `eu-west-1`.
/// Cluster, proxy and China endpoints have the same shape.
fn region_from_host(host: &str) -> Option<String> {
    let rest = host
        .strip_suffix(".rds.amazonaws.com")
        .or_else(|| host.strip_suffix(".rds.amazonaws.com.cn"))?;
    let region = rest.rsplit('.').next()?;
    let looks_like_region = region.contains('-') && region.ends_with(|c: char| c.is_ascii_digit());
    looks_like_region.then(|| region.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector of `aws-sdk-rds`'s own `signing_works` test, so the token matches what the AWS
    /// SDKs produce.
    #[test]
    fn signs_like_the_aws_sdk() {
        let target = Target {
            host: "prod-instance.us-east-1.rds.amazonaws.com",
            port: 3306,
            user: "peccy",
        };
        let credentials = Credentials::new("akid", "secret", None, None, "test");
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_724_709_600);
        assert_eq!(
            sign(&target, "us-east-1", credentials, time).unwrap(),
            "prod-instance.us-east-1.rds.amazonaws.com:3306/?Action=connect&DBUser=peccy&X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=akid%2F20240826%2Fus-east-1%2Frds-db%2Faws4_request&X-Amz-Date=20240826T220000Z&X-Amz-Expires=900&X-Amz-SignedHeaders=host&X-Amz-Signature=dd0cba843009474347af724090233265628ace491ea17ce3eb3da098b983ad89"
        );
    }

    #[test]
    fn missing_credentials_read_as_words() {
        let err = CredentialsError::not_loaded("no providers in chain provided credentials");
        let text = no_credentials(Some("prod"), &cause(&err));
        assert_eq!(
            text,
            "AWS credentials for profile “prod” could not be loaded: none was found in the environment, ~/.aws or an SSO session. Sign in with `aws sso login --profile prod`, or check ~/.aws/config and ~/.aws/credentials."
        );
    }

    #[test]
    fn region_comes_from_rds_host_names() {
        let cases = [
            ("db.abc123.eu-west-1.rds.amazonaws.com", Some("eu-west-1")),
            (
                "app.cluster-ro-abc.ap-southeast-1.rds.amazonaws.com",
                Some("ap-southeast-1"),
            ),
            ("p.proxy-abc.us-east-2.rds.amazonaws.com", Some("us-east-2")),
            ("db.abc.cn-north-1.rds.amazonaws.com.cn", Some("cn-north-1")),
            ("db.example.com", None),
            ("127.0.0.1", None),
        ];
        for (host, region) in cases {
            assert_eq!(region_from_host(host).as_deref(), region, "{host}");
        }
    }
}
