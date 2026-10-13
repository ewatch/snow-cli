//! Decode a JSON.parse string literal from native UI bootstrap HTML, without
//! evaluating JavaScript or exposing the rest of the page (which contains tokens).
use serde_json::Value;

pub(super) fn parse_bootstrap(body: &str) -> Option<Value> {
    let rest = body
        .split_once("uxGlobals.session = JSON.parse(")?
        .1
        .trim_start();
    let quote = rest.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let mut chars = rest[quote.len_utf8()..].chars();
    let mut decoded = String::new();
    while let Some(ch) = chars.next() {
        if ch == quote {
            return serde_json::from_str(&decoded).ok();
        }
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        match chars.next()? {
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000c}'),
            'u' => {
                let digits: String = chars.by_ref().take(4).collect();
                let code = u16::from_str_radix(&digits, 16).ok()?;
                if (0xD800..=0xDBFF).contains(&code) {
                    if chars.next()? != '\\' || chars.next()? != 'u' {
                        return None;
                    }
                    let low: String = chars.by_ref().take(4).collect();
                    let low = u16::from_str_radix(&low, 16).ok()?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    decoded.push(char::from_u32(
                        0x10000 + ((u32::from(code) - 0xD800) << 10) + u32::from(low) - 0xDC00,
                    )?);
                } else {
                    decoded.push(char::from_u32(u32::from(code))?);
                }
            }
            '\n' => {}
            ch => decoded.push(ch),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_ui_json_without_executing_scripts() {
        let html = r#"uxGlobals.session = JSON.parse('{\"output\":{\"user\":{\"name\":\"O\'Brien\",\"dateFormat\":\"yyyy\-MM\-dd\",\"icon\":\"\uD83D\uDE00\"}}}'); malicious();"#;
        let data = parse_bootstrap(html).unwrap();
        assert_eq!(data["output"]["user"]["name"], "O'Brien");
        assert_eq!(data["output"]["user"]["dateFormat"], "yyyy-MM-dd");
        assert_eq!(data["output"]["user"]["icon"], "😀");
        assert!(parse_bootstrap("login form").is_none());
        assert!(parse_bootstrap("uxGlobals.session = JSON.parse('broken');").is_none());
    }
}
