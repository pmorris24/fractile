//! Pure parser and resolver for `fractile://` deep links.
//!
//! Supported links (case-insensitive `fractile://` scheme):
//! - `fractile://settings` — open Settings on the default pane
//! - `fractile://settings/<pane>` — open a Settings pane (`general`, `rules`,
//!   `workspaces`, `shortcuts`, `labs`, `layout`, `activity`, `whats-new`)
//! - `fractile://settings/<pane>/<focus>` — open a Settings pane and highlight
//!   a specific sub-feature or setting row
//! - `fractile://labs` / `fractile://labs/<slug>` — open Settings › Labs and
//!   highlight `<slug>` (for example `scrolling-strip`, `playground`)
//! - `fractile://features/<slug>` — map a feature slug to its Settings pane
//!   and optional highlight target
//!
//! Security invariant: deep links **only** open Settings and highlight a pane,
//! setting, or Lab card. They never mutate `config.toml`, never enable a Lab,
//! and never run tiling or destructive commands.

use std::fmt;

/// The registered macOS URL scheme for Fractile.
pub const SCHEME: &str = "fractile";

/// Maximum accepted raw link length in bytes.
pub const MAX_LINK_LEN: usize = 2048;

/// Maximum accepted length for a single host, pane, or slug segment.
pub const MAX_SEGMENT_LEN: usize = 64;

/// Settings panes addressable by `fractile://` links.
///
/// Includes panes that may not be registered in every build (`Labs`, `Layout`,
/// `WhatsNew`) so links parse deterministically and fall back cleanly to the
/// default Settings pane while preserving their requested highlight target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeepLinkPane {
    General,
    Rules,
    Workspaces,
    Shortcuts,
    Labs,
    Layout,
    Activity,
    WhatsNew,
}

impl DeepLinkPane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Rules => "rules",
            Self::Workspaces => "workspaces",
            Self::Shortcuts => "shortcuts",
            Self::Labs => "labs",
            Self::Layout => "layout",
            Self::Activity => "activity",
            Self::WhatsNew => "whats-new",
        }
    }

    /// Parses a normalized lowercase pane slug (including accepted aliases).
    #[must_use]
    pub fn parse(slug: &str) -> Option<Self> {
        match slug {
            "general" => Some(Self::General),
            "rules" => Some(Self::Rules),
            "workspaces" => Some(Self::Workspaces),
            "shortcuts" => Some(Self::Shortcuts),
            "labs" => Some(Self::Labs),
            "layout" => Some(Self::Layout),
            "activity" => Some(Self::Activity),
            "whats-new" | "what-s-new" => Some(Self::WhatsNew),
            _ => None,
        }
    }
}

/// A validated `fractile://` deep link.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeepLink {
    /// `fractile://settings[/<pane>[/<focus>]]`
    Settings {
        pane: Option<DeepLinkPane>,
        /// Preserved when the link named an unrecognized pane slug.
        unknown_pane: Option<String>,
        focus: Option<String>,
    },
    /// `fractile://labs[/<slug>]`
    Labs { slug: Option<String> },
    /// `fractile://features/<slug>`
    Feature { slug: String },
    /// A syntactically clean `fractile://<host>` whose host is unrecognized;
    /// resolves to default Settings with a diagnostic note.
    UnknownHost { host: String },
}

/// The resolved UI destination passed to Settings when a deep link opens.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpenTarget {
    /// Target Settings pane, or `None` for the default pane.
    pub pane: Option<DeepLinkPane>,
    /// Optional Lab slug to highlight/focus in Settings › Labs.
    pub highlight_lab: Option<String>,
    /// Optional setting or feature slug to highlight/focus in the target pane.
    pub highlight_feature: Option<String>,
    /// Diagnostic message to log when a link named an unknown host, pane, or slug.
    pub diagnostic: Option<String>,
}

/// Why a raw URL or CLI target was rejected before touching the UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeepLinkError {
    Empty,
    TooLong,
    UnsafeCharacters,
    InvalidScheme,
    MissingHost,
    InvalidHost,
    InvalidSegment,
    TooManySegments,
}

impl fmt::Display for DeepLinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("deep link is empty"),
            Self::TooLong => f.write_str("deep link exceeds maximum length"),
            Self::UnsafeCharacters => f.write_str("deep link contains disallowed characters"),
            Self::InvalidScheme => f.write_str("deep link must use the fractile:// scheme"),
            Self::MissingHost => f.write_str("deep link is missing a target host"),
            Self::InvalidHost => f.write_str("deep link host must use only [a-z0-9-]"),
            Self::InvalidSegment => f.write_str("deep link path segment must use only [a-z0-9-]"),
            Self::TooManySegments => f.write_str("deep link has too many path segments"),
        }
    }
}

impl DeepLink {
    /// Formats the link in canonical lowercase `fractile://...` form (no query
    /// string or fragment).
    #[must_use]
    pub fn canonical_url(&self) -> String {
        match self {
            Self::Settings {
                pane: Some(pane),
                focus: Some(focus),
                ..
            } => format!("{SCHEME}://settings/{}/{focus}", pane.as_str()),
            Self::Settings {
                pane: Some(pane),
                focus: None,
                ..
            } => format!("{SCHEME}://settings/{}", pane.as_str()),
            Self::Settings {
                pane: None,
                unknown_pane: Some(unknown),
                focus: Some(focus),
            } => format!("{SCHEME}://settings/{unknown}/{focus}"),
            Self::Settings {
                pane: None,
                unknown_pane: Some(unknown),
                focus: None,
            } => format!("{SCHEME}://settings/{unknown}"),
            Self::Settings {
                pane: None,
                unknown_pane: None,
                ..
            } => format!("{SCHEME}://settings"),
            Self::Labs { slug: Some(slug) } => format!("{SCHEME}://labs/{slug}"),
            Self::Labs { slug: None } => format!("{SCHEME}://labs"),
            Self::Feature { slug } => format!("{SCHEME}://features/{slug}"),
            Self::UnknownHost { host } => format!("{SCHEME}://{host}"),
        }
    }

    /// Resolves this link into an [`OpenTarget`] for the Settings window.
    #[must_use]
    pub fn resolve(&self) -> OpenTarget {
        match self {
            Self::Settings {
                pane: Some(DeepLinkPane::Labs),
                focus,
                ..
            } => OpenTarget {
                pane: Some(DeepLinkPane::Labs),
                highlight_lab: focus.clone(),
                highlight_feature: focus.clone(),
                diagnostic: None,
            },
            Self::Settings {
                pane: Some(pane),
                focus,
                ..
            } => OpenTarget {
                pane: Some(*pane),
                highlight_lab: None,
                highlight_feature: focus.clone(),
                diagnostic: None,
            },
            Self::Settings {
                pane: None,
                unknown_pane: Some(name),
                focus,
            } => OpenTarget {
                pane: None,
                highlight_lab: None,
                highlight_feature: focus.clone(),
                diagnostic: Some(format!(
                    "unknown settings pane '{name}'; opening default Settings"
                )),
            },
            Self::Settings {
                pane: None,
                unknown_pane: None,
                focus,
            } => OpenTarget {
                pane: None,
                highlight_lab: None,
                highlight_feature: focus.clone(),
                diagnostic: None,
            },
            Self::Labs { slug } => OpenTarget {
                pane: Some(DeepLinkPane::Labs),
                highlight_lab: slug.clone(),
                highlight_feature: slug.clone(),
                diagnostic: None,
            },
            Self::Feature { slug } => resolve_feature(slug),
            Self::UnknownHost { host } => OpenTarget {
                pane: None,
                highlight_lab: None,
                highlight_feature: None,
                diagnostic: Some(format!(
                    "unknown deep link host '{host}'; opening default Settings"
                )),
            },
        }
    }
}

fn resolve_feature(slug: &str) -> OpenTarget {
    match slug {
        "scrolling-strip" | "playground" => OpenTarget {
            pane: Some(DeepLinkPane::Labs),
            highlight_lab: Some(slug.to_owned()),
            highlight_feature: Some(slug.to_owned()),
            diagnostic: None,
        },
        "workspaces" => OpenTarget {
            pane: Some(DeepLinkPane::Workspaces),
            highlight_lab: None,
            highlight_feature: None,
            diagnostic: None,
        },
        "monitors" | "displays" => OpenTarget {
            pane: Some(DeepLinkPane::Workspaces),
            highlight_lab: None,
            highlight_feature: Some(slug.to_owned()),
            diagnostic: None,
        },
        "rules" => OpenTarget {
            pane: Some(DeepLinkPane::Rules),
            highlight_lab: None,
            highlight_feature: None,
            diagnostic: None,
        },
        "app-rules" => OpenTarget {
            pane: Some(DeepLinkPane::Rules),
            highlight_lab: None,
            highlight_feature: Some("rules".to_owned()),
            diagnostic: None,
        },
        "shortcuts" | "hotkeys" | "keybindings" => OpenTarget {
            pane: Some(DeepLinkPane::Shortcuts),
            highlight_lab: None,
            highlight_feature: (slug != "shortcuts").then(|| slug.to_owned()),
            diagnostic: None,
        },
        "activity" | "journal" | "doctor" => OpenTarget {
            pane: Some(DeepLinkPane::Activity),
            highlight_lab: None,
            highlight_feature: (slug != "activity").then(|| slug.to_owned()),
            diagnostic: None,
        },
        "gaps" | "max-width" | "layout" => OpenTarget {
            pane: Some(DeepLinkPane::Layout),
            highlight_lab: None,
            highlight_feature: Some(slug.to_owned()),
            diagnostic: None,
        },
        "hud" | "workspace-hud" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("hud".to_owned()),
            diagnostic: None,
        },
        "tiling" | "automatic-tiling" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("tiling".to_owned()),
            diagnostic: None,
        },
        "pause-on-builtin-display" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("pause-on-builtin-display".to_owned()),
            diagnostic: None,
        },
        "login" | "launch-at-login" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("login".to_owned()),
            diagnostic: None,
        },
        "setup" | "setup-tour" | "onboarding" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("setup-tour".to_owned()),
            diagnostic: None,
        },
        "config" => OpenTarget {
            pane: Some(DeepLinkPane::General),
            highlight_lab: None,
            highlight_feature: Some("config".to_owned()),
            diagnostic: None,
        },
        _ => OpenTarget {
            pane: None,
            highlight_lab: None,
            highlight_feature: Some(slug.to_owned()),
            diagnostic: Some(format!(
                "unknown feature '{slug}'; opening default Settings"
            )),
        },
    }
}

/// Parses a full `fractile://...` URL from macOS or another external caller.
///
/// Query strings and fragments are ignored safely. Percent-encoded bytes,
/// control characters, path traversal (`..`), userinfo (`@`), ports (`:`),
/// and non-`[a-z0-9-]` tokens are rejected cleanly.
pub fn parse_url(raw: &str) -> Result<DeepLink, DeepLinkError> {
    if raw.is_empty() {
        return Err(DeepLinkError::Empty);
    }
    if raw.len() > MAX_LINK_LEN {
        return Err(DeepLinkError::TooLong);
    }
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(DeepLinkError::UnsafeCharacters);
    }

    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err(DeepLinkError::InvalidScheme);
    };
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return Err(DeepLinkError::InvalidScheme);
    }

    // Ignore `#fragment` and `?query` safely; never echo them into logs or UI.
    let before_fragment = rest.split_once('#').map_or(rest, |(head, _)| head);
    let authority_and_path = before_fragment
        .split_once('?')
        .map_or(before_fragment, |(head, _)| head);

    let (raw_host, raw_path) = match authority_and_path.split_once('/') {
        Some((host, path)) => (host, path),
        None => (authority_and_path, ""),
    };

    if raw_host.is_empty() {
        return Err(DeepLinkError::MissingHost);
    }
    let host = normalize_slug(raw_host).ok_or(DeepLinkError::InvalidHost)?;

    let trimmed_path = raw_path.trim_end_matches('/');
    let mut segments = Vec::new();
    if !trimmed_path.is_empty() {
        for segment in trimmed_path.split('/') {
            let normalized = normalize_slug(segment).ok_or(DeepLinkError::InvalidSegment)?;
            segments.push(normalized);
        }
    }

    match host.as_str() {
        "settings" => match segments.as_slice() {
            [] => Ok(DeepLink::Settings {
                pane: None,
                unknown_pane: None,
                focus: None,
            }),
            [pane_slug] => Ok(settings_link(pane_slug, None)),
            [pane_slug, focus_slug] => Ok(settings_link(pane_slug, Some(focus_slug.clone()))),
            _ => Err(DeepLinkError::TooManySegments),
        },
        "labs" => match segments.as_slice() {
            [] => Ok(DeepLink::Labs { slug: None }),
            [slug] => Ok(DeepLink::Labs {
                slug: Some(slug.clone()),
            }),
            _ => Err(DeepLinkError::TooManySegments),
        },
        "features" => match segments.as_slice() {
            [] => Ok(DeepLink::UnknownHost { host }),
            [slug] => Ok(DeepLink::Feature { slug: slug.clone() }),
            _ => Err(DeepLinkError::TooManySegments),
        },
        _ => {
            if segments.len() > 2 {
                return Err(DeepLinkError::TooManySegments);
            }
            Ok(DeepLink::UnknownHost { host })
        }
    }
}

/// Parses either a full `fractile://...` URL or a CLI shorthand target such as
/// `settings/workspaces`, `labs/scrolling-strip`, `features/gaps`, `labs`,
/// `settings`, or a bare pane name (`workspaces`, `rules`, `general`, ...).
pub fn parse_input(raw: &str) -> Result<DeepLink, DeepLinkError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DeepLinkError::Empty);
    }
    if trimmed.contains("://") {
        return parse_url(trimmed);
    }
    if trimmed.contains(':') || trimmed.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(DeepLinkError::InvalidScheme);
    }
    let normalized_first = trimmed.split('/').next().and_then(normalize_slug);
    let url = match normalized_first.as_deref() {
        Some("settings" | "labs" | "features") => format!("{SCHEME}://{trimmed}"),
        Some(first) if DeepLinkPane::parse(first).is_some() => {
            format!("{SCHEME}://settings/{trimmed}")
        }
        _ => format!("{SCHEME}://{trimmed}"),
    };
    parse_url(&url)
}

fn settings_link(pane_slug: &str, focus: Option<String>) -> DeepLink {
    match DeepLinkPane::parse(pane_slug) {
        Some(pane) => DeepLink::Settings {
            pane: Some(pane),
            unknown_pane: None,
            focus,
        },
        None => DeepLink::Settings {
            pane: None,
            unknown_pane: Some(pane_slug.to_owned()),
            focus,
        },
    }
}

/// Accepts non-empty ASCII `[a-z0-9-]` slugs (case-insensitive) up to
/// [`MAX_SEGMENT_LEN`], rejecting leading/trailing hyphens, percent escapes,
/// dots, spaces, and punctuation.
fn normalize_slug(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_SEGMENT_LEN {
        return None;
    }
    let bytes = raw.as_bytes();
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
        return None;
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
    {
        return None;
    }
    Some(raw.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_settings_urls() {
        assert_eq!(
            parse_url("fractile://settings").unwrap(),
            DeepLink::Settings {
                pane: None,
                unknown_pane: None,
                focus: None,
            }
        );
        assert_eq!(
            parse_url("FRACTILE://Settings/").unwrap(),
            DeepLink::Settings {
                pane: None,
                unknown_pane: None,
                focus: None,
            }
        );
        for (url, expected_pane) in [
            ("fractile://settings/general", DeepLinkPane::General),
            ("fractile://settings/rules", DeepLinkPane::Rules),
            ("fractile://settings/workspaces", DeepLinkPane::Workspaces),
            ("fractile://settings/shortcuts", DeepLinkPane::Shortcuts),
            ("fractile://settings/labs", DeepLinkPane::Labs),
            ("fractile://settings/layout", DeepLinkPane::Layout),
            ("fractile://settings/activity", DeepLinkPane::Activity),
            ("fractile://settings/whats-new", DeepLinkPane::WhatsNew),
            ("fractile://settings/what-s-new", DeepLinkPane::WhatsNew),
            ("FrAcTiLe://Settings/Workspaces/", DeepLinkPane::Workspaces),
        ] {
            let parsed = parse_url(url).unwrap();
            assert_eq!(
                parsed,
                DeepLink::Settings {
                    pane: Some(expected_pane),
                    unknown_pane: None,
                    focus: None,
                },
                "{url}"
            );
            assert_eq!(parsed.resolve().pane, Some(expected_pane));
            assert_eq!(parsed.resolve().diagnostic, None);
        }
    }

    #[test]
    fn parses_settings_sub_feature_focus() {
        let parsed = parse_url("fractile://settings/general/hud").unwrap();
        assert_eq!(parsed.canonical_url(), "fractile://settings/general/hud");
        assert_eq!(
            parsed.resolve(),
            OpenTarget {
                pane: Some(DeepLinkPane::General),
                highlight_lab: None,
                highlight_feature: Some("hud".into()),
                diagnostic: None,
            }
        );

        let lab_via_settings = parse_url("fractile://settings/labs/scrolling-strip").unwrap();
        assert_eq!(
            lab_via_settings.resolve(),
            OpenTarget {
                pane: Some(DeepLinkPane::Labs),
                highlight_lab: Some("scrolling-strip".into()),
                highlight_feature: Some("scrolling-strip".into()),
                diagnostic: None,
            }
        );
    }

    #[test]
    fn parses_labs_urls() {
        assert_eq!(
            parse_url("fractile://labs").unwrap(),
            DeepLink::Labs { slug: None }
        );
        assert_eq!(
            parse_url("fractile://labs/").unwrap(),
            DeepLink::Labs { slug: None }
        );
        let strip = parse_url("fractile://labs/Scrolling-Strip/").unwrap();
        assert_eq!(
            strip,
            DeepLink::Labs {
                slug: Some("scrolling-strip".into()),
            }
        );
        assert_eq!(strip.canonical_url(), "fractile://labs/scrolling-strip");
        assert_eq!(
            strip.resolve(),
            OpenTarget {
                pane: Some(DeepLinkPane::Labs),
                highlight_lab: Some("scrolling-strip".into()),
                highlight_feature: Some("scrolling-strip".into()),
                diagnostic: None,
            }
        );
    }

    #[test]
    fn parses_per_feature_urls() {
        let cases = [
            (
                "fractile://features/scrolling-strip",
                Some(DeepLinkPane::Labs),
                Some("scrolling-strip"),
                Some("scrolling-strip"),
            ),
            (
                "fractile://features/workspaces",
                Some(DeepLinkPane::Workspaces),
                None,
                None,
            ),
            (
                "fractile://features/rules",
                Some(DeepLinkPane::Rules),
                None,
                None,
            ),
            (
                "fractile://features/shortcuts",
                Some(DeepLinkPane::Shortcuts),
                None,
                None,
            ),
            (
                "fractile://features/gaps",
                Some(DeepLinkPane::Layout),
                None,
                Some("gaps"),
            ),
            (
                "fractile://features/hud",
                Some(DeepLinkPane::General),
                None,
                Some("hud"),
            ),
        ];
        for (url, pane, lab, feature) in cases {
            let target = parse_url(url).unwrap().resolve();
            assert_eq!(target.pane, pane, "{url}");
            assert_eq!(target.highlight_lab.as_deref(), lab, "{url}");
            assert_eq!(target.highlight_feature.as_deref(), feature, "{url}");
            assert_eq!(target.diagnostic, None, "{url}");
        }
    }

    #[test]
    fn ignores_query_params_and_fragments_safely() {
        let parsed =
            parse_url("fractile://labs/scrolling-strip?utm_source=x&enable=true#top").unwrap();
        assert_eq!(
            parsed,
            DeepLink::Labs {
                slug: Some("scrolling-strip".into()),
            }
        );
        assert_eq!(parsed.canonical_url(), "fractile://labs/scrolling-strip");

        let settings = parse_url("fractile://settings/workspaces?ref=github&reset=1").unwrap();
        assert_eq!(
            settings,
            DeepLink::Settings {
                pane: Some(DeepLinkPane::Workspaces),
                unknown_pane: None,
                focus: None,
            }
        );
        assert_eq!(settings.canonical_url(), "fractile://settings/workspaces");
    }

    #[test]
    fn falls_back_cleanly_on_unknown_host_pane_or_feature() {
        let unknown_pane = parse_url("fractile://settings/not-a-pane")
            .unwrap()
            .resolve();
        assert_eq!(unknown_pane.pane, None);
        assert!(
            unknown_pane
                .diagnostic
                .as_deref()
                .is_some_and(|msg| msg.contains("not-a-pane"))
        );

        let unknown_host = parse_url("fractile://mystery").unwrap().resolve();
        assert_eq!(unknown_host.pane, None);
        assert!(
            unknown_host
                .diagnostic
                .as_deref()
                .is_some_and(|msg| msg.contains("mystery"))
        );

        let unknown_feature = parse_url("fractile://features/future-thing")
            .unwrap()
            .resolve();
        assert_eq!(unknown_feature.pane, None);
        assert_eq!(
            unknown_feature.highlight_feature.as_deref(),
            Some("future-thing")
        );
        assert!(
            unknown_feature
                .diagnostic
                .as_deref()
                .is_some_and(|msg| msg.contains("future-thing"))
        );
    }

    #[test]
    fn rejects_invalid_schemes_and_malformed_urls() {
        for bad in [
            "",
            "http://settings/general",
            "https://github.com/pmorris24/fractile/labs/scrolling-strip",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "hex://labs/live-text",
            "fractile:settings/general",
            "fractile://",
            "fractile:///settings",
            "fractile://user@settings",
            "fractile://settings:8080/general",
            "fractile://labs/a/b",
            "fractile://settings/general/hud/extra",
            "fractile://labs//scrolling-strip",
        ] {
            assert!(parse_url(bad).is_err(), "should reject: {bad:?}");
        }
        let oversized = format!("fractile://labs/{}", "a".repeat(MAX_LINK_LEN));
        assert_eq!(parse_url(&oversized), Err(DeepLinkError::TooLong));
    }

    #[test]
    fn rejects_percent_encoded_and_injection_strings() {
        for hostile in [
            "fractile://labs/%FF",
            "fractile://labs/%2e%2e",
            "fractile://labs/..%2fetc",
            "fractile://labs/..",
            "fractile://settings/general/..%2fsecrets",
            "fractile://labs/<script>alert(1)</script>",
            "fractile://labs/scrolling-strip%0aquit",
            "fractile://labs/scrolling-strip\nquit",
            "fractile://labs/scrolling-strip\r\nquit",
            "fractile://labs/scrolling-strip\0",
            "fractile://labs/scrolling strip",
            "fractile://labs/scrolling;rm-rf",
            "fractile://labs/scrolling\\strip",
            "fractile://labs/'or'1'='1",
            "fractile://labs/-leading-dash",
            "fractile://labs/trailing-dash-",
            "fractile://settings/general/hud?a=1\nquit",
        ] {
            assert!(parse_url(hostile).is_err(), "should reject: {hostile:?}");
        }
    }

    #[test]
    fn parses_cli_shorthand_and_full_urls() {
        assert_eq!(
            parse_input("fractile://labs/scrolling-strip").unwrap(),
            DeepLink::Labs {
                slug: Some("scrolling-strip".into()),
            }
        );
        assert_eq!(
            parse_input("labs/scrolling-strip").unwrap(),
            DeepLink::Labs {
                slug: Some("scrolling-strip".into()),
            }
        );
        assert_eq!(
            parse_input("settings/workspaces").unwrap(),
            DeepLink::Settings {
                pane: Some(DeepLinkPane::Workspaces),
                unknown_pane: None,
                focus: None,
            }
        );
        assert_eq!(
            parse_input("workspaces").unwrap(),
            DeepLink::Settings {
                pane: Some(DeepLinkPane::Workspaces),
                unknown_pane: None,
                focus: None,
            }
        );
        assert_eq!(
            parse_input("features/gaps").unwrap(),
            DeepLink::Feature {
                slug: "gaps".into(),
            }
        );
        assert!(parse_input("javascript:alert(1)").is_err());
        assert!(parse_input("https://evil.example/labs").is_err());
    }
}
