//! Path templates: `${placeholder}` text resolved against roots at runtime.

use super::*;

/// A location written with `${...}` placeholders, resolved at run time.
///
/// `${installDir}` is the directory holding the launcher binary; every other
/// name is an environment variable. Both separators are accepted and
/// normalised for the running OS.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PathTemplate(String);

impl PathTemplate {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Placeholder names used by this template, in order of appearance.
    pub fn placeholders(&self) -> Vec<String> {
        let mut names = Vec::new();
        let bytes = self.0.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'$' && bytes[i + 1] == b'{' {
                if let Some(end) = self.0[i + 2..].find('}') {
                    names.push(self.0[i + 2..i + 2 + end].to_string());
                    i = i + 2 + end + 1;
                    continue;
                }
            }
            i += 1;
        }
        names
    }

    /// Checks the template can be resolved at all: balanced placeholders,
    /// usable names, and no way to climb out of the sandbox.
    pub fn validate(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        if self.0.trim().is_empty() {
            return Err(DescriptorError::new(
                source,
                field,
                "expected a non-empty path template, found an empty string",
            ));
        }

        let mut rest = self.0.as_str();
        while let Some(start) = rest.find("${") {
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected every `${{` to be closed by `}}`, found `{}`",
                        self.0
                    ),
                ));
            };
            let name = &after[..end];
            if name.is_empty() {
                return Err(DescriptorError::new(
                    source,
                    field,
                    "expected a name inside `${}`, found an empty placeholder",
                ));
            }
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')'))
            {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected a placeholder name of letters, digits, `_`, `(` or `)`, found `{name}`"
                    ),
                ));
            }
            rest = &after[end + 1..];
        }

        for part in self.0.split(['/', '\\']) {
            if part == ".." {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected a path that stays inside its roots, found a `..` segment in `{}`",
                        self.0
                    ),
                ));
            }
        }

        Ok(())
    }
}

/// One location, or several tried in order.
///
/// Written as a plain string in the ordinary case. An array means the launcher
/// keeps the same thing in one of several places depending on how it was
/// installed: the first candidate that exists wins, and the first one listed is
/// used when none do, so a file that does not exist yet is created where the
/// launcher expects it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PathSpec {
    One(PathTemplate),
    FirstExisting(Vec<PathTemplate>),
}

impl PathSpec {
    pub fn candidates(&self) -> &[PathTemplate] {
        match self {
            PathSpec::One(template) => std::slice::from_ref(template),
            PathSpec::FirstExisting(templates) => templates,
        }
    }

    /// Placeholder names used by any candidate.
    pub fn placeholders(&self) -> Vec<String> {
        self.candidates()
            .iter()
            .flat_map(|template| template.placeholders())
            .collect()
    }

    pub(super) fn validate(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        match self {
            PathSpec::One(template) => template.validate(source, field),
            PathSpec::FirstExisting(templates) => {
                if templates.is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        field,
                        "expected at least one path template, found an empty list",
                    ));
                }
                for (index, template) in templates.iter().enumerate() {
                    template.validate(source, &format!("{field}[{index}]"))?;
                }
                Ok(())
            }
        }
    }
}
