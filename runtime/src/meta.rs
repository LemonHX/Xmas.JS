//! `import.meta`, for the parts of it that need only the module's own path.
//!
//! The engine parses `import.meta` and evaluates it to `undefined`: it has the syntax and no
//! object behind it. Until it grows one, the uses this runtime can answer are rewritten from the
//! source before the engine ever sees them.
//!
//! What that costs, said plainly:
//!
//! - `import.meta.url` becomes a string literal, so `import.meta` on its own is still `undefined`,
//!   and `const m = import.meta; m.url` does not work.
//! - The rewrite is textual (by span, not by re-printing the tree), so a module that uses
//!   `import.meta` pays one extra parse even when it is plain JavaScript.
//!
//! The properties are the ones Bun documents. Each is answered from the file's own path, so a
//! module read from disk says where it is on disk and a module defined in memory says so.

use std::path::Path;

use oxc::allocator::Allocator;
use oxc::ast::ast::{Expression, MemberExpression};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::{Parser, ParseOptions};
use oxc::span::SourceType;

/// One replacement: the bytes to overwrite, and what to put there.
struct Splice {
    start: usize,
    end: usize,
    text: String,
}

struct Scan<'a> {
    /// The file this code came from, as a `file://` URL.
    url: &'a str,
    found: Vec<Splice>,
    /// A use this layer cannot answer, kept so the caller can say which one.
    unknown: Option<&'static str>,
}

impl<'a> Visit<'a> for Scan<'a> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        if let Expression::StaticMemberExpression(member) = expression
            && let Expression::MetaProperty(meta) = &member.object
            && meta.meta.name == "import"
            && meta.property.name == "meta"
        {
            let name = member.property.name.as_str();
            match value(name, self.url) {
                Some(text) => self.found.push(Splice {
                    start: meta.span.start as usize,
                    end: member.span.end as usize,
                    text,
                }),
                None => self.unknown = Some(name_of(name)),
            }
            // `import.meta.url` holds no further expression, so nothing below needs looking at.
            return;
        }
        walk::walk_expression(self, expression);
    }
}

/// The properties this layer answers, spelled as Bun does.
fn value(name: &str, url: &str) -> Option<String> {
    let path = url.strip_prefix("file://")?;
    let directory = match path.rsplit_once('/') {
        Some((directory, _)) => directory,
        None => return None,
    };
    let file = path.rsplit('/').next().unwrap_or_default();
    let text = match name {
        "url" => url.to_owned(),
        "path" | "filename" => path.to_owned(),
        "dir" | "dirname" => directory.to_owned(),
        "file" => file.to_owned(),
        _ => return None,
    };
    Some(quote(&text))
}

fn name_of(name: &str) -> &'static str {
    // Only the fixed set is ever reported; the text itself is not needed to say "unknown".
    let _ = name;
    "an import.meta property this runtime does not implement yet"
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a path is valid JS string text")
}

/// `file://` for a path on disk. The path is already absolute: the loader only ever compiles a
/// module it has resolved, and resolution produces absolute paths.
pub fn url_of(path: &str) -> String {
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        // Windows: `C:\\a\\b` becomes `file:///C:/a/b`.
        format!("file:///{}", path.replace('\\', "/"))
    }
}

/// Rewrite the `import.meta` uses in `source` that this layer can answer. `None` when there is
/// nothing to do, which is every module that does not mention `import.meta` at all.
pub fn rewrite(source: &str, path: &str) -> Result<Option<String>, String> {
    if !source.contains("import.meta") && !source.contains("import\n.meta") {
        return Ok(None);
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::unambiguous())
        .with_options(ParseOptions {
            parse_regular_expression: false,
            allow_return_outside_function: true,
            ..ParseOptions::default()
        })
        .parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        // Not this layer's diagnostic to give: the compile step or the engine will say what is
        // wrong with the file, and it can say it with a position.
        return Ok(None);
    }
    let url = url_of(Path::new(path).to_str().unwrap_or(path));
    let mut scan = Scan {
        url: &url,
        found: Vec::new(),
        unknown: None,
    };
    scan.visit_program(&parsed.program);
    if let Some(name) = scan.unknown {
        return Err(format!("{path}: {name}"));
    }
    if scan.found.is_empty() {
        return Ok(None);
    }
    // Back to front, so an earlier offset is still valid after a later one was replaced.
    let mut out = source.to_owned();
    for splice in scan.found.iter().rev() {
        if splice.end > out.len() {
            continue;
        }
        out.replace_range(splice.start..splice.end, &splice.text);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_the_file_itself() {
        assert_eq!(
            rewrite("export const u = import.meta.url;", "/a/b/c.js").unwrap().as_deref(),
            Some("export const u = \"file:///a/b/c.js\";")
        );
    }

    #[test]
    fn the_path_group_is_answered() {
        let code = "const a = import.meta.dir, b = import.meta.file, c = import.meta.path;";
        assert_eq!(
            rewrite(code, "/a/b/c.js").unwrap().as_deref(),
            Some("const a = \"/a/b\", b = \"c.js\", c = \"/a/b/c.js\";")
        );
    }

    #[test]
    fn several_uses_keep_their_order() {
        let code = "const x = [import.meta.url, import.meta.file];";
        assert_eq!(
            rewrite(code, "/a/c.js").unwrap().as_deref(),
            Some("const x = [\"file:///a/c.js\", \"c.js\"];")
        );
    }

    #[test]
    fn a_module_that_never_mentions_it_is_left_alone() {
        assert!(rewrite("export const x = 1;", "/a/b.js").unwrap().is_none());
    }

    #[test]
    fn a_property_this_layer_cannot_answer_is_named() {
        let error = rewrite("const m = import.meta.main;", "/a/b.js").unwrap_err();
        assert!(error.contains("does not implement yet"), "{error}");
    }
}
