//! Shared adblock engine and webview-facing request decisions.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use adblock::{
    Engine,
    filters::cosmetic::{CosmeticFilterMask, CosmeticFilterOperator},
    lists::{FilterSet, ParseOptions, ParsedLine, parse_filter},
    request::Request,
};

/// Request kinds reported by webviews and mapped to adblock request types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// Top-level page navigation, which is always allowed.
    Document,
    /// An embedded frame.
    Subdocument,
    /// A CSS stylesheet.
    Stylesheet,
    /// A JavaScript resource.
    Script,
    /// An image resource.
    Image,
    /// A font resource.
    Font,
    /// An audio or video resource.
    Media,
    /// An XMLHttpRequest request.
    XmlHttpRequest,
    /// A Fetch API request, treated as XMLHttpRequest by the engine.
    Fetch,
    /// A WebSocket connection.
    WebSocket,
    /// A beacon or hyperlink auditing request.
    Ping,
    /// Any other request kind.
    Other,
}

impl ResourceKind {
    fn request_type(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Subdocument => "subdocument",
            Self::Stylesheet => "stylesheet",
            Self::Script => "script",
            Self::Image => "image",
            Self::Font => "font",
            Self::Media => "media",
            Self::XmlHttpRequest | Self::Fetch => "xmlhttprequest",
            Self::WebSocket => "websocket",
            Self::Ping => "ping",
            Self::Other => "other",
        }
    }
}

/// A webview request decision after applying blocking rules and exceptions.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Whether to cancel the request.
    pub block: bool,
    /// Replacement data URL supplied by the engine, if a resource is available.
    pub redirect: Option<String>,
}

/// A thread-safe, immutable filter engine with an atomic blocked-request counter.
pub struct Blocker {
    engine: Arc<Engine>,
    generic_selectors: Vec<String>,
    rule_count: usize,
    blocked: AtomicU64,
}

impl Blocker {
    /// Builds an engine from list texts, ignoring comments and unparseable rules.
    /// The rule count includes successfully parsed network and cosmetic rules before deduplication.
    pub fn from_lists(lists: &[String]) -> Blocker {
        let mut filters = FilterSet::new(false);
        let mut rule_count = 0;
        let mut generic_selectors = Vec::new();
        for list in lists {
            for line in list.lines() {
                let Ok(parsed) = parse_filter(line, false, ParseOptions::default()) else {
                    continue;
                };
                rule_count += 1;
                if let ParsedLine::Cosmetic(rule) = parsed
                    && (!rule.has_hostname_constraint() || rule.hidden_generic_rule().is_some())
                    && !rule.mask.contains(CosmeticFilterMask::UNHIDE)
                    && rule.action.is_none()
                    && let [CosmeticFilterOperator::CssSelector(selector)] =
                        rule.selector.as_slice()
                    && selector.starts_with(['.', '#'])
                {
                    // The engine otherwise requires DOM class/ID keys to return these generic rules.
                    generic_selectors.push(selector.clone());
                }
            }
            filters.add_filter_list(list.clone(), ParseOptions::default());
        }
        generic_selectors.sort_unstable();
        generic_selectors.dedup();
        Self {
            engine: Arc::new(Engine::new_with_filter_set(filters)),
            generic_selectors,
            rule_count,
            blocked: AtomicU64::new(0),
        }
    }

    /// Creates an engine that blocks nothing, for use before lists are loaded.
    pub fn empty() -> Blocker {
        Self::from_lists(&[])
    }

    /// Checks a GET request, preserving engine exceptions and important-rule precedence.
    /// Top-level documents and malformed request or source URLs are always allowed.
    pub fn check(&self, url: &str, source_url: &str, kind: ResourceKind) -> Verdict {
        if kind == ResourceKind::Document
            || Request::new(source_url, source_url, "document", "GET").is_err()
        {
            return Verdict::default();
        }
        let Ok(request) = Request::new(url, source_url, kind.request_type(), "GET") else {
            return Verdict::default();
        };
        let result = self.engine.check_network_request(&request);
        let block = result.should_block();
        if block {
            self.blocked.fetch_add(1, Ordering::Relaxed);
        }
        Verdict {
            block,
            redirect: result.redirect,
        }
    }

    /// Like [`Blocker::cosmetic_css`] but only rules written for this site, without the
    /// thousands of generic selectors. Much smaller and far less likely to hide real UI.
    pub fn site_css(&self, page_url: &str) -> String {
        if Request::new(page_url, page_url, "document", "GET").is_err() {
            return String::new();
        }
        let resources = self.engine.url_cosmetic_resources(page_url);
        let generic: std::collections::HashSet<&String> = self.generic_selectors.iter().collect();
        selectors_to_css(
            resources
                .hide_selectors
                .into_iter()
                .filter(|s| !generic.contains(s))
                .collect(),
        )
    }

    /// Returns hiding CSS in rules of at most 500 selectors, including generic class/ID rules.
    /// Cosmetic exceptions are respected; selectors with braces or `</` are discarded.
    /// Invalid page URLs produce an empty string.
    pub fn cosmetic_css(&self, page_url: &str) -> String {
        if Request::new(page_url, page_url, "document", "GET").is_err() {
            return String::new();
        }
        let mut resources = self.engine.url_cosmetic_resources(page_url);
        if !resources.generichide {
            resources.hide_selectors.extend(
                self.generic_selectors
                    .iter()
                    .filter(|selector| !resources.exceptions.contains(*selector))
                    .cloned(),
            );
        }
        selectors_to_css(resources.hide_selectors.into_iter().collect())
    }

    /// Returns the total number of successfully parsed network and cosmetic rules.
    pub fn rule_count(&self) -> usize {
        self.rule_count
    }

    /// Returns how many calls to [`Self::check`] have blocked a request.
    pub fn blocked_count(&self) -> u64 {
        self.blocked.load(Ordering::Relaxed)
    }
}

fn selectors_to_css(mut selectors: Vec<String>) -> String {
    selectors.retain(|selector| !selector.contains(['{', '}']) && !selector.contains("</"));
    selectors.sort_unstable();
    selectors.dedup();
    let mut css = String::new();
    for chunk in selectors.chunks(500) {
        css.push_str(&chunk.join(","));
        css.push_str(" { display: none !important; }\n");
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    const AD: &str = "https://ads.example.com/ad.js";
    const PAGE: &str = "https://publisher.test/";

    fn blocker(text: &str) -> Blocker {
        Blocker::from_lists(&[text.to_owned()])
    }

    #[test]
    fn site_css_keeps_site_rules_and_drops_generic_ones() {
        let b = blocker(
            "example.com##.site-ad
##.generic-ad
",
        );
        let site = b.site_css("https://example.com/");
        assert!(site.contains(".site-ad"), "{site}");
        assert!(!site.contains(".generic-ad"), "{site}");
        assert!(
            b.cosmetic_css("https://example.com/")
                .contains(".generic-ad")
        );
        assert!(b.site_css("https://other.com/").is_empty());
    }

    #[test]
    fn blocks_third_party_ad_script() {
        assert!(
            blocker("||ads.example.com^")
                .check(AD, PAGE, ResourceKind::Script)
                .block
        );
    }

    #[test]
    fn allows_unrelated_url() {
        assert!(
            !blocker("||ads.example.com^")
                .check("https://cdn.test/app.js", PAGE, ResourceKind::Script)
                .block
        );
    }

    #[test]
    fn respects_exceptions() {
        let blocker = blocker("||ads.example.com^\n@@||ads.example.com/ad.js");
        assert!(!blocker.check(AD, PAGE, ResourceKind::Script).block);
    }

    #[test]
    fn important_overrides_exception() {
        let blocker = blocker("||ads.example.com^$important\n@@||ads.example.com^");
        assert!(blocker.check(AD, PAGE, ResourceKind::Script).block);
    }

    #[test]
    fn restricts_rules_to_source_domain() {
        let blocker = blocker("||ads.example.com^$domain=publisher.test");
        assert!(blocker.check(AD, PAGE, ResourceKind::Script).block);
        assert!(
            !blocker
                .check(AD, "https://other.test", ResourceKind::Script)
                .block
        );
    }

    #[test]
    fn image_option_matches_only_images() {
        let blocker = blocker("||ads.example.com^$image");
        assert!(blocker.check(AD, PAGE, ResourceKind::Image).block);
        assert!(!blocker.check(AD, PAGE, ResourceKind::Script).block);
    }

    #[test]
    fn script_option_matches_only_scripts() {
        let blocker = blocker("||ads.example.com^$script");
        assert!(blocker.check(AD, PAGE, ResourceKind::Script).block);
        assert!(!blocker.check(AD, PAGE, ResourceKind::Image).block);
    }

    #[test]
    fn maps_all_other_resource_types() {
        for (kind, option, url) in [
            (ResourceKind::Subdocument, "subdocument", AD),
            (ResourceKind::Stylesheet, "stylesheet", AD),
            (ResourceKind::Font, "font", AD),
            (ResourceKind::Media, "media", AD),
            (ResourceKind::XmlHttpRequest, "xmlhttprequest", AD),
            (ResourceKind::Fetch, "xmlhttprequest", AD),
            (
                ResourceKind::WebSocket,
                "websocket",
                "wss://ads.example.com/socket",
            ),
            (ResourceKind::Ping, "ping", AD),
            (ResourceKind::Other, "other", AD),
        ] {
            assert!(
                blocker(&format!("||ads.example.com^${option}"))
                    .check(url, PAGE, kind)
                    .block,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn document_is_always_allowed() {
        let blocker = blocker("||ads.example.com^$important");
        assert_eq!(
            blocker.check(AD, AD, ResourceKind::Document),
            Verdict::default()
        );
        assert_eq!(blocker.blocked_count(), 0);
    }

    #[test]
    fn malformed_urls_are_allowed() {
        let blocker = blocker("||ads.example.com^");
        for invalid in ["", "not a url", "https://", "\0"] {
            assert_eq!(
                blocker.check(invalid, PAGE, ResourceKind::Script),
                Verdict::default()
            );
            assert_eq!(
                blocker.check(AD, invalid, ResourceKind::Script),
                Verdict::default()
            );
            assert!(blocker.cosmetic_css(invalid).is_empty());
        }
    }

    #[test]
    fn cosmetics_are_domain_specific() {
        let blocker = blocker("example.com##.ad-banner");
        assert_eq!(
            blocker.cosmetic_css("https://example.com"),
            ".ad-banner { display: none !important; }\n"
        );
        assert!(blocker.cosmetic_css("https://other.com").is_empty());
    }

    #[test]
    fn includes_generic_cosmetics() {
        let blocker = blocker("##.sponsored\n###advert\n##.sponsor > span\n##[data-ad]");
        let css = blocker.cosmetic_css(PAGE);
        for selector in [".sponsored", "#advert", ".sponsor > span", "[data-ad]"] {
            assert!(css.contains(selector), "{css}");
        }
    }

    #[test]
    fn respects_cosmetic_exceptions_and_negated_domains() {
        let blocker = blocker("##.sponsored\nexample.com#@#.sponsored\n~example.com##.negated");
        assert!(blocker.cosmetic_css("https://example.com").is_empty());
        let css = blocker.cosmetic_css("https://other.com");
        assert!(css.contains(".sponsored"));
        assert!(css.contains(".negated"));
    }

    #[test]
    fn respects_generichide() {
        let blocker = blocker("##.sponsored\nexample.com##.specific\n@@||example.com^$generichide");
        assert_eq!(
            blocker.cosmetic_css("https://example.com"),
            ".specific { display: none !important; }\n"
        );
    }

    #[test]
    fn chunks_css_into_at_most_500_selectors() {
        let list = (0..1001)
            .map(|n| format!("example.com##.ad{n}\n"))
            .collect::<String>();
        let css = blocker(&list).cosmetic_css("https://example.com");
        let counts: Vec<_> = css
            .lines()
            .map(|line| line.split_once(" {").unwrap().0.split(',').count())
            .collect();
        assert_eq!(counts, [500, 500, 1]);
    }

    #[test]
    fn skips_css_and_html_injection() {
        let css = selectors_to_css(
            [".safe", ".ad{color:red}", ".ad}", "</style><script>"]
                .map(str::to_owned)
                .to_vec(),
        );
        assert_eq!(css, ".safe { display: none !important; }\n");
        let blocker = blocker("example.com##[data-ad='{']\nexample.com##[data-ad='</style>']");
        assert!(blocker.cosmetic_css("https://example.com").is_empty());
    }

    #[test]
    fn empty_blocks_nothing() {
        let blocker = Blocker::empty();
        assert_eq!(
            blocker.check(AD, PAGE, ResourceKind::Script),
            Verdict::default()
        );
        assert!(blocker.cosmetic_css(PAGE).is_empty());
        assert_eq!(blocker.rule_count(), 0);
    }

    #[test]
    fn counts_only_blocked_requests() {
        let blocker = blocker("||ads.example.com^");
        blocker.check(AD, PAGE, ResourceKind::Script);
        blocker.check(AD, PAGE, ResourceKind::Image);
        blocker.check(PAGE, PAGE, ResourceKind::Script);
        assert_eq!(blocker.blocked_count(), 2);
    }

    #[test]
    fn counts_parsed_rules_across_lists() {
        let blocker = Blocker::from_lists(&[
            "! comment\n[Adblock Plus 2.0]\n||ads.example.com^\nexample.com##.ad\n".into(),
            "@@||ads.example.com/allowed\n##.generic\nexample.com#@#.ad\n||bad.test^$unknown-option".into(),
        ]);
        assert_eq!(blocker.rule_count(), 5);
    }

    #[test]
    fn blocker_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Blocker>();
    }
}
