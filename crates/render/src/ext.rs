//! Builders for sections whose options are optional, lists that may be empty, and flags.

use uniwrt_uci::{Section, Value};

pub(crate) trait SectionExt: Sized {
    /// Sets the option when there is a value.
    #[must_use]
    fn maybe(self, name: &str, value: Option<impl Into<Value>>) -> Self;
    /// Sets a list of the items, or nothing when there are none.
    #[must_use]
    fn list<S: Into<String>>(self, name: &str, items: impl IntoIterator<Item = S>) -> Self;
    /// Sets `'1'` when on; leaves the option out (its default, off) when not.
    #[must_use]
    fn flag(self, name: &str, on: bool) -> Self;
}

impl SectionExt for Section {
    fn maybe(self, name: &str, value: Option<impl Into<Value>>) -> Self {
        match value {
            Some(value) => self.with(name, value),
            None => self,
        }
    }

    fn list<S: Into<String>>(self, name: &str, items: impl IntoIterator<Item = S>) -> Self {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        if items.is_empty() {
            self
        } else {
            self.with(name, Value::List(items))
        }
    }

    fn flag(self, name: &str, on: bool) -> Self {
        if on { self.with(name, "1") } else { self }
    }
}
