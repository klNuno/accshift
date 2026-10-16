//! `ThemeEditor.svelte`'s script: the fields of the theme being edited, the
//! draft document they make, its issues, and the save.
//!
//! Mounted fresh per open, as the `{#if editing}` block does: the fields
//! start from the document handed in and the prop is never read again. While
//! the editor is open the app paints the draft (`previewThemeDocument`);
//! closing without a save puts back the theme of the settings
//! (`endThemePreview`), a save keeps what the preview painted
//! (`commitThemePreview`).

use std::collections::BTreeMap;

use accshift_app::i18n::{self, Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_theme::{
    is_built_in_theme, resolve_theme_surface_opacities, resolve_theme_tokens, root_tokens,
    serialize_theme_document, validate_theme_document, ColorScheme, IssueLevel, PendingSave,
    ResolvedTokens, ThemeDocument, ThemeIssue, ThemeIssueCode, ThemeRegistry, TokenKey, TokenKind,
};

/// `BLOCKING_CODES`: problems that cannot be saved, each one would be
/// dropped on the next load.
const BLOCKING: [ThemeIssueCode; 3] = [
    ThemeIssueCode::InvalidValue,
    ThemeIssueCode::UnsafeCss,
    ThemeIssueCode::CssTooLong,
];

/// `String.prototype.trim`: Unicode white space and the BOM.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// What the editor's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeEditorEvent {
    /// Cancel, Escape or a click on the overlay.
    Cancel,
    Save,
    /// "Copy JSON": the draft to the clipboard.
    Export,
}

/// One option of the base select.
#[derive(Clone, Debug, PartialEq)]
pub struct BaseOption {
    pub id: String,
    pub label: String,
}

/// How a save ended.
#[derive(Debug)]
pub enum SaveOutcome {
    /// Written: the registry holds the document now.
    Saved(ThemeDocument),
    /// The write failed: the editor stays open on its fields.
    Failed,
}

pub struct ThemeEditor {
    initial: ThemeDocument,
    pub name: String,
    pub author: String,
    pub version: String,
    pub color_scheme: ColorScheme,
    /// `base`: the id of the theme this one extends, `""` for none.
    pub base: String,
    pub glass: bool,
    pub tokens: BTreeMap<TokenKey, String>,
    pub css: String,
    /// `saveError`: the last save failed.
    pub save_error: bool,
    /// `backgroundOpacity` of the settings, for the contrast check.
    pub background_opacity: f64,
    /// Bumped per open: animations and focus start over.
    pub mount: u64,
    /// `.editor-body`'s `scrollTop`.
    pub scroll: f32,
    /// The CSS field's own `scrollTop`.
    pub css_scroll: f32,
    save_job: Option<(Pending<Result<(), String>>, PendingSave)>,
}

impl ThemeEditor {
    pub fn new(source: ThemeDocument, background_opacity: f64, mount: u64) -> ThemeEditor {
        ThemeEditor {
            name: source.name.clone(),
            author: source.author.clone().unwrap_or_default(),
            version: source.version.clone().unwrap_or_default(),
            color_scheme: source.color_scheme,
            base: source.extends.clone().unwrap_or_default(),
            glass: source.glass == Some(true),
            tokens: source.tokens.clone(),
            css: source.css.clone().unwrap_or_default(),
            save_error: false,
            background_opacity,
            mount,
            scroll: 0.0,
            css_scroll: 0.0,
            save_job: None,
            initial: source,
        }
    }

    /// `startFromSelected`: a custom theme opens as it is; a built-in is
    /// read only, so editing one means editing a copy of it.
    pub fn start_from(
        registry: &ThemeRegistry,
        theme_id: &str,
        locale: Locale,
        background_opacity: f64,
        mount: u64,
    ) -> ThemeEditor {
        let current = registry.theme(Some(theme_id));
        if !is_built_in_theme(&current.id) {
            return ThemeEditor::new(current.document, background_opacity, mount);
        }
        let label = i18n::translate(locale, current.label_key);
        let name = i18n::translate_with(
            locale,
            "settings.themeCopyName",
            &[("name", TranslationValue::Str(label))],
        );
        let id = registry.suggest_theme_id(&name);
        let copy = accshift_theme::duplicate_theme_document(&current.document, &id, &name);
        ThemeEditor::new(copy, background_opacity, mount)
    }

    pub fn id(&self) -> &str {
        &self.initial.id
    }

    /// `draft`: the document the fields make.
    pub fn draft(&self) -> ThemeDocument {
        let trimmed = |s: &str| {
            let t = js_trim(s);
            (!t.is_empty()).then(|| t.to_owned())
        };
        ThemeDocument {
            schema_version: self.initial.schema_version,
            id: self.initial.id.clone(),
            name: trimmed(&self.name).unwrap_or_else(|| self.initial.id.clone()),
            author: trimmed(&self.author),
            version: trimmed(&self.version),
            color_scheme: self.color_scheme,
            extends: (!self.base.is_empty()).then(|| self.base.clone()),
            glass: self.glass.then_some(true),
            tokens: self.tokens.clone(),
            css: (!js_trim(&self.css).is_empty()).then(|| self.css.clone()),
        }
    }

    /// `resolved`: the draft flattened over what it extends.
    pub fn resolved(&self, registry: &ThemeRegistry, draft: &ThemeDocument) -> ResolvedTokens {
        resolve_theme_tokens(draft, |id| registry.document(id), root_tokens())
    }

    /// `issues`, against card surfaces painted at the draft's card alpha.
    pub fn issues(
        &self,
        registry: &ThemeRegistry,
        draft: &ThemeDocument,
        resolved: &ResolvedTokens,
    ) -> Vec<ThemeIssue> {
        let theme = registry.theme_from_document(draft);
        let card_alpha =
            resolve_theme_surface_opacities(&theme, self.background_opacity, true).card_opacity;
        validate_theme_document(draft, resolved, Some(card_alpha))
    }

    /// `blocked`: an issue that cannot be saved.
    pub fn blocked(issues: &[ThemeIssue]) -> bool {
        issues.iter().any(|issue| BLOCKING.contains(&issue.code))
    }

    /// `baseOptions`: every theme but this one and those that would loop
    /// back into it.
    pub fn base_options(&self, registry: &ThemeRegistry, locale: Locale) -> Vec<BaseOption> {
        fn reaches(registry: &ThemeRegistry, start: &ThemeDocument, target: &str) -> bool {
            let mut document = Some(start);
            for _ in 0..=8 {
                let Some(d) = document else {
                    return false;
                };
                if d.id == target {
                    return true;
                }
                document = d.extends.as_deref().and_then(|id| registry.document(id));
            }
            false
        }
        let own = self.initial.id.as_str();
        registry
            .all_themes()
            .into_iter()
            .filter(|t| t.id != own && !reaches(registry, &t.document, own))
            .map(|t| BaseOption {
                label: if t.is_custom {
                    t.display_name.clone()
                } else {
                    i18n::translate(locale, t.label_key)
                },
                id: t.id,
            })
            .collect()
    }

    /// `inheritedFrom`: the theme a token currently comes from.
    pub fn inherited_from(
        registry: &ThemeRegistry,
        resolved: &ResolvedTokens,
        key: TokenKey,
        locale: Locale,
    ) -> String {
        for id in resolved.chain.iter().skip(1) {
            if let Some(document) = registry.document(id) {
                if document.tokens.contains_key(&key) {
                    return document.name.clone();
                }
            }
        }
        i18n::translate(locale, "themeEditor.builtInBase")
    }

    pub fn set_token(&mut self, key: TokenKey, value: String) {
        self.tokens.insert(key, value);
    }

    pub fn reset_token(&mut self, key: TokenKey) {
        self.tokens.remove(&key);
    }

    /// `copyToClipboard`: the draft as a theme file.
    pub fn export(&self) -> String {
        serialize_theme_document(&self.draft())
    }

    pub fn saving(&self) -> bool {
        self.save_job.is_some()
    }

    /// `save`: writes the draft through the backend. Refused while blocked
    /// or already writing.
    pub fn save(&mut self, registry: &ThemeRegistry, jobs: &Jobs) {
        if self.save_job.is_some() {
            return;
        }
        let draft = self.draft();
        let resolved = self.resolved(registry, &draft);
        if Self::blocked(&self.issues(registry, &draft, &resolved)) {
            return;
        }
        self.save_error = false;
        let pending = match registry.prepare_save(&draft, true) {
            Ok(pending) => pending,
            Err(_) => {
                self.save_error = true;
                return;
            }
        };
        let payload = pending.payload.clone();
        let job = jobs.run(move |backend| {
            let theme: accshift_app::types::CustomTheme = serde_json::to_value(&payload)
                .and_then(serde_json::from_value)
                .map_err(|e| e.to_string())?;
            backend.save_custom_theme(&theme).map_err(|e| e.message)
        });
        self.save_job = Some((job, pending));
    }

    /// The save's end, once: the registry takes the document on success.
    pub fn poll_save(&mut self, registry: &mut ThemeRegistry) -> Option<SaveOutcome> {
        let (job, _) = self.save_job.as_mut()?;
        let result = match job.poll() {
            Poll::Waiting => return None,
            Poll::Ready(result) => result,
            Poll::Gone => Err("the save job died".to_owned()),
        };
        let (_, pending) = self.save_job.take()?;
        match result {
            Ok(()) => {
                let document = self.draft();
                registry.finish_save(pending);
                Some(SaveOutcome::Saved(document))
            }
            Err(_) => {
                self.save_error = true;
                Some(SaveOutcome::Failed)
            }
        }
    }
}

impl super::AppModel {
    /// Once per frame: the save's end, and the preview following the
    /// draft. Once the editor is gone without a save, the settings' theme
    /// comes back (`onDestroy`: `endThemePreview`).
    pub(super) fn tick_theme_editor(&mut self, now: std::time::Instant) {
        let reduced = self.os_reduced_motion;
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let editor = self
            .settings_panel
            .as_mut()
            .and_then(|p| p.theme_editor.as_mut());
        let Some(editor) = editor else {
            if b.theme_preview.take().is_some() {
                b.theme = super::compute_theme(&b.registry, &b.stores, b.runtime_os, reduced);
            }
            return;
        };
        if let Some(SaveOutcome::Saved(document)) = editor.poll_save(&mut b.registry) {
            // `onSaved`, then `commitThemePreview`: the saved theme is the
            // selected one and the app keeps what the preview painted.
            let mut store_ctx = b.stores.context().clone();
            store_ctx.theme_ids = b.registry.all_themes().into_iter().map(|t| t.id).collect();
            b.stores.set_context(store_ctx);
            b.theme_preview = None;
            if let Some(panel) = self.settings_panel.as_mut() {
                panel.theme_editor = None;
                panel.draft.theme_id = document.id.clone();
            }
            let theme = b.registry.theme(Some(&document.id));
            let key = super::theme_key(&b.stores, &b.registry, b.runtime_os);
            b.theme = super::theme_state(key, theme, reduced);
            super::settings_panel::toast(
                &mut self.toasts,
                "settings.themeSaved",
                accshift_app::actions::toasts::ToastType::Info,
                now,
            );
            return;
        }
        let draft = editor.draft();
        if b.theme_preview.as_ref() != Some(&draft) {
            b.theme = super::preview_theme(&b.registry, &b.stores, b.runtime_os, reduced, &draft);
            b.theme_preview = Some(draft);
        }
    }
}

/// `GROUP_LABELS`.
pub fn group_label(group: accshift_theme::TokenGroup) -> &'static str {
    use accshift_theme::TokenGroup as G;
    match group {
        G::Surface => "themeGroup.surface",
        G::Text => "themeGroup.text",
        G::Semantic => "themeGroup.semantic",
        G::Shape => "themeGroup.shape",
        G::Typography => "themeGroup.typography",
        G::Motion => "themeGroup.motion",
    }
}

/// `KIND_LABELS`.
fn kind_label(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::RgbTriplet => "themeKind.rgbTriplet",
        TokenKind::HexColor => "themeKind.hexColor",
        TokenKind::Color => "themeKind.color",
        TokenKind::Length => "themeKind.length",
        TokenKind::SignedLength => "themeKind.signedLength",
        TokenKind::Shadow => "themeKind.shadow",
        TokenKind::Gradient => "themeKind.gradient",
        TokenKind::Number => "themeKind.number",
        TokenKind::Choice => "themeKind.choice",
        TokenKind::FontStack => "themeKind.fontStack",
    }
}

/// `choiceLabel`: choices that are words of ours get a translation, CSS
/// keywords stay as typed in the theme file.
pub fn choice_label(locale: Locale, key: TokenKey, choice: &str) -> String {
    let message = match (key, choice) {
        (TokenKey::FontSmoothing, "auto") => Some("themeSmoothing.auto"),
        (TokenKey::FontSmoothing, "none") => Some("themeSmoothing.none"),
        (_, "compact") => Some("themeDensity.compact"),
        (_, "cozy") => Some("themeDensity.cozy"),
        (_, "comfortable") => Some("themeDensity.comfortable"),
        (_, "circle") => Some("themeAvatarShape.circle"),
        (_, "rounded") => Some("themeAvatarShape.rounded"),
        (_, "square") => Some("themeAvatarShape.square"),
        _ => None,
    };
    message.map_or_else(|| choice.to_owned(), |m| i18n::translate(locale, m))
}

/// `tokenLabel`.
pub fn token_label(locale: Locale, key: TokenKey) -> String {
    i18n::translate(locale, &key.spec().label_key())
}

/// `issueText`.
pub fn issue_text(locale: Locale, issue: &ThemeIssue) -> String {
    let token = |key: Option<TokenKey>| {
        TranslationValue::Str(key.map(|k| token_label(locale, k)).unwrap_or_default())
    };
    let num = |n: Option<f64>| n.map_or(TranslationValue::Null, TranslationValue::Num);
    match issue.code {
        ThemeIssueCode::InvalidValue => {
            let kind = issue.expected.unwrap_or(TokenKind::Color);
            i18n::translate_with(
                locale,
                "themeEditor.issueInvalidValue",
                &[
                    ("token", token(issue.token)),
                    (
                        "kind",
                        TranslationValue::Str(i18n::translate(locale, kind_label(kind))),
                    ),
                    ("example", TranslationValue::Str(kind.example().to_owned())),
                ],
            )
        }
        ThemeIssueCode::UnknownToken => i18n::translate_with(
            locale,
            "themeEditor.issueUnknownToken",
            &[(
                "token",
                TranslationValue::Str(issue.token.map(|k| k.name().to_owned()).unwrap_or_default()),
            )],
        ),
        ThemeIssueCode::MissingToken => i18n::translate_with(
            locale,
            "themeEditor.issueMissingToken",
            &[("token", token(issue.token))],
        ),
        ThemeIssueCode::UnknownBase => i18n::translate_with(
            locale,
            "themeEditor.issueUnknownBase",
            &[(
                "base",
                TranslationValue::Str(issue.base.clone().unwrap_or_default()),
            )],
        ),
        ThemeIssueCode::UnsafeCss => i18n::translate_with(
            locale,
            "themeEditor.issueUnsafeCss",
            &[(
                "construct",
                TranslationValue::Str(issue.construct.unwrap_or_default().to_owned()),
            )],
        ),
        ThemeIssueCode::CssTooLong => i18n::translate_with(
            locale,
            "themeEditor.issueCssTooLong",
            &[
                ("length", num(issue.length.map(|n| n as f64))),
                ("limit", num(issue.limit.map(|n| n as f64))),
            ],
        ),
        ThemeIssueCode::Contrast => i18n::translate_with(
            locale,
            "themeEditor.issueContrast",
            &[
                ("token", token(issue.token)),
                ("against", token(issue.against)),
                ("ratio", num(issue.ratio)),
                ("target", num(issue.target)),
            ],
        ),
    }
}

/// `issueByToken`: the issue a row shows, the last one for its token.
pub fn issue_for(issues: &[ThemeIssue], key: TokenKey) -> Option<&ThemeIssue> {
    issues.iter().rev().find(|issue| issue.token == Some(key))
}

/// `issue.level === "error"`.
pub fn is_error(issue: &ThemeIssue) -> bool {
    issue.level == IssueLevel::Error
}

/// `toHex`: the colour input's value, `#000000` when the text is no colour.
pub fn to_hex(value: &str) -> String {
    let Some(rgb) = accshift_theme::contrast::parse_color(value) else {
        return "#000000".into();
    };
    let channel = |n: f64| n.clamp(0.0, 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgb.r),
        channel(rgb.g),
        channel(rgb.b)
    )
}

/// `fromHex`: what a picked colour writes into a token of `kind`.
pub fn from_hex(hex: &str, kind: TokenKind) -> String {
    if kind != TokenKind::RgbTriplet {
        return hex.to_owned();
    }
    match accshift_theme::contrast::parse_color(hex) {
        Some(rgb) => format!("{} {} {}", rgb.r, rgb.g, rgb.b),
        None => hex.to_owned(),
    }
}

/// `COLOR_KINDS`: tokens with a colour input before their text field.
pub fn is_color_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RgbTriplet | TokenKind::HexColor | TokenKind::Color
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> (ThemeRegistry, ThemeEditor) {
        let registry = ThemeRegistry::new();
        let editor = ThemeEditor::start_from(&registry, "dark", Locale::En, 100.0, 1);
        (registry, editor)
    }

    #[test]
    fn a_built_in_opens_as_a_copy_that_extends_it() {
        let (_, editor) = editor();
        let draft = editor.draft();
        assert_eq!(draft.extends.as_deref(), Some("dark"));
        assert!(draft.tokens.is_empty());
        assert!(!is_built_in_theme(&draft.id));
        assert_eq!(draft.name, "Dark copy");
    }

    #[test]
    fn a_blank_name_falls_back_to_the_id_and_blank_fields_drop() {
        let (_, mut editor) = editor();
        editor.name = "   ".into();
        editor.author = " ".into();
        editor.css = "\n".into();
        let draft = editor.draft();
        assert_eq!(draft.name, draft.id);
        assert_eq!(draft.author, None);
        assert_eq!(draft.css, None);
    }

    #[test]
    fn an_invalid_value_blocks_the_save_and_flags_its_row() {
        let (registry, mut editor) = editor();
        editor.set_token(TokenKey::RadiusSm, "big".into());
        let draft = editor.draft();
        let resolved = editor.resolved(&registry, &draft);
        let issues = editor.issues(&registry, &draft, &resolved);
        assert!(ThemeEditor::blocked(&issues));
        let issue = issue_for(&issues, TokenKey::RadiusSm).expect("row issue");
        assert!(is_error(issue));
        assert!(issue_text(Locale::En, issue).contains("8px"));
        editor.reset_token(TokenKey::RadiusSm);
        let draft = editor.draft();
        let resolved = editor.resolved(&registry, &draft);
        assert!(!ThemeEditor::blocked(
            &editor.issues(&registry, &draft, &resolved)
        ));
    }

    #[test]
    fn the_base_select_leaves_out_the_theme_itself() {
        let (registry, editor) = editor();
        let options = editor.base_options(&registry, Locale::En);
        assert!(options.iter().all(|o| o.id != editor.id()));
        assert!(options.iter().any(|o| o.id == "dark" && o.label == "Dark"));
    }

    #[test]
    fn hex_round_trips_through_the_token_kinds() {
        assert_eq!(to_hex("9 9 11"), "#09090b");
        assert_eq!(to_hex("nope"), "#000000");
        assert_eq!(from_hex("#ff0080", TokenKind::RgbTriplet), "255 0 128");
        assert_eq!(from_hex("#ff0080", TokenKind::HexColor), "#ff0080");
    }
}
