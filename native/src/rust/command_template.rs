//! Explicit parameter substitution into a draft. No shell evaluation or automatic quoting.
use crate::bridge::completion::completion_ffi::{
    CompletionContext, TemplateFields, TemplatePreview,
};

#[derive(Debug)]
enum Part<'a> {
    Text(&'a str),
    Parameter(usize),
}

fn parse(text: &str) -> Result<(Vec<String>, Vec<Part<'_>>), u8> {
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(3);
    }
    let mut names = Vec::new();
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("{{") {
        let literal = &rest[..at];
        if literal.contains("}}") {
            return Err(3);
        }
        parts.push(Part::Text(literal));
        let tail = &rest[at + 2..];
        let end = tail.find("}}").ok_or(3)?;
        let name = &tail[..end];
        if name.is_empty()
            || name.len() > 32
            || !name
                .bytes()
                .enumerate()
                .all(|(i, c)| c.is_ascii_alphabetic() || c == b'_' || (i > 0 && c.is_ascii_digit()))
        {
            return Err(3);
        }
        let index = match names.iter().position(|s| s == name) {
            Some(index) => index,
            None if names.len() < 16 => {
                names.push(name.to_owned());
                names.len() - 1
            }
            None => return Err(3),
        };
        parts.push(Part::Parameter(index));
        rest = &tail[end + 2..];
    }
    if rest.contains("}}") || names.is_empty() {
        return Err(3);
    }
    parts.push(Part::Text(rest));
    Ok((names, parts))
}
pub fn command_template_fields(text: &str) -> TemplateFields {
    match parse(text) {
        Ok((names, _)) => TemplateFields { names, error: 0 },
        Err(error) => TemplateFields {
            names: vec![],
            error,
        },
    }
}
fn expand(text: &str, values: &[String]) -> Result<String, u8> {
    let (names, parts) = parse(text)?;
    if values.len() != names.len()
        || values
            .iter()
            .any(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
    {
        return Err(4);
    }
    let mut result = String::new();
    for part in parts {
        result.push_str(match part {
            Part::Text(s) => s,
            Part::Parameter(index) => &values[index],
        });
        if result.len() > 4096 {
            return Err(4);
        }
    }
    Ok(result)
}
pub fn command_template_render(
    context: &CompletionContext,
    id: u32,
    values: &[String],
) -> TemplatePreview {
    let render = || -> Result<String, u8> {
        if !super::command_completion::completion_valid(context) {
            return Err(1);
        }
        let catalog = super::command_library::command_catalog();
        let entry = catalog.entries.iter().find(|e| e.id == id).ok_or(2)?;
        if entry.is_template {
            expand(&entry.command, values)
        } else if values.is_empty() {
            Ok(entry.command.clone())
        } else {
            Err(4)
        }
    };
    match render() {
        Ok(text) => TemplatePreview { text, error: 0 },
        Err(error) => TemplatePreview {
            text: String::new(),
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_templates_preserve_literal_shell_text_repeat_parameters_and_bound_substitution() {
        let text = "echo '${HOME}' '{{name}}' {{name}} {{second}}";
        assert_eq!(command_template_fields(text).names, ["name", "second"]);
        assert_eq!(
            expand(text, &["中文 $x; & literal".into(), "{{name}}".into()]).unwrap(),
            "echo '${HOME}' '中文 $x; & literal' 中文 $x; & literal {{name}}"
        );
        for invalid in [
            "no fields",
            "{{1bad}}",
            "{{}}",
            "{{a}",
            "}} {{a}}",
            "{{a{{b}}",
            "{{a}}\n",
        ] {
            assert!(parse(invalid).is_err());
        }
        assert!(parse(&(0..17).map(|i| format!("{{{{p{i}}}}}")).collect::<String>()).is_err());
        for values in [
            vec![],
            vec!["".into()],
            vec!["\r".into()],
            vec!["x".repeat(4097)],
        ] {
            assert!(expand("{{a}}", &values).is_err());
        }
        assert!(expand("{{a}}{{a}}", &["x".repeat(2049)]).is_err());
    }
}
