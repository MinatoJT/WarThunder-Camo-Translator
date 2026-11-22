use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Language {
    #[default]
    Chinese,
    English,
}

impl Language {
    pub fn text<'a>(self, chinese: &'a str, english: &'a str) -> &'a str {
        match self {
            Self::Chinese => chinese,
            Self::English => english,
        }
    }
}

// Retain both versions of messages that can outlive a language change.
pub(super) struct Message {
    chinese: String,
    english: Option<String>,
}

impl Message {
    pub fn new(chinese: impl Into<String>, english: impl Into<String>) -> Self {
        Self {
            chinese: chinese.into(),
            english: Some(english.into()),
        }
    }

    pub fn diagnostic(text: impl Into<String>) -> Self {
        Self {
            chinese: text.into(),
            english: None,
        }
    }

    pub fn text(&self, language: Language) -> &str {
        language.text(
            &self.chinese,
            self.english.as_deref().unwrap_or(&self.chinese),
        )
    }
}
