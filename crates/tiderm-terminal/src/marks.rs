//! Independent, bounded per-pane rules. Last rule wins overlapping cells.
use crate::{matching::Matcher, search::SearchOptions};

pub const MAX_RULES: usize = 16;
pub const MAX_PATTERN_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkSpec {
    pub pattern: String,
    pub options: SearchOptions,
    pub color: u8,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkRule {
    pub id: u64,
    pub spec: MarkSpec,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkedCells {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub color: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MarkError {
    Pattern = 1,
    Regex,
    Limit,
    Missing,
    Selection,
    Color = 9,
    Duplicate = 10,
    NoMatch = 11,
}

pub(crate) struct Rule {
    pub row: MarkRule,
    pub matcher: Matcher,
}
#[derive(Default)]
pub(crate) struct Marks {
    pub rules: Vec<Rule>,
    pub serial: u64,
}

impl Marks {
    fn compile(spec: &MarkSpec) -> Result<Matcher, MarkError> {
        if spec.pattern.trim().is_empty()
            || spec.pattern.len() > MAX_PATTERN_BYTES
            || spec.pattern.chars().any(char::is_control)
        {
            return Err(MarkError::Pattern);
        }
        if spec.color >= 6 {
            return Err(MarkError::Color);
        }
        Matcher::new(&spec.pattern, spec.options, Some(64 * 1024)).map_err(|_| MarkError::Regex)
    }
    pub fn put(&mut self, id: u64, spec: MarkSpec) -> Result<u64, MarkError> {
        let matcher = Self::compile(&spec)?;
        if self.rules.iter().any(|r| {
            r.row.id != id
                && r.row.spec.pattern == spec.pattern
                && r.row.spec.options == spec.options
        }) {
            return Err(MarkError::Duplicate);
        }
        if id != 0 {
            let rule = self
                .rules
                .iter_mut()
                .find(|r| r.row.id == id)
                .ok_or(MarkError::Missing)?;
            *rule = Rule {
                row: MarkRule { id, spec },
                matcher,
            };
            return Ok(id);
        }
        if self.rules.len() >= MAX_RULES {
            return Err(MarkError::Limit);
        }
        self.serial = self.serial.checked_add(1).ok_or(MarkError::Limit)?;
        let id = self.serial;
        self.rules.push(Rule {
            row: MarkRule { id, spec },
            matcher,
        });
        Ok(id)
    }
    pub fn replace(&mut self, specs: &[MarkSpec]) -> Result<(), MarkError> {
        if specs.len() > MAX_RULES {
            return Err(MarkError::Limit);
        }
        let mut next = Self {
            rules: Vec::new(),
            serial: self.serial,
        };
        for spec in specs {
            next.put(0, spec.clone())?;
        }
        *self = next;
        Ok(())
    }
}

pub fn validate_rules(specs: &[MarkSpec]) -> Result<(), MarkError> {
    Marks::default().replace(specs)
}
