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
use oxc::ast::ast::Expression;
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
///
/// A remote module answers the same group from its URL: there is no path on disk for it, and the
/// URL is the identity the program wrote and the one its relative imports resolve against.
fn value(name: &str, url: &str) -> Option<String> {
    // 'dir' and 'dirname' are the same thing here; Bun spells both and so does this.
    if name != "url" && name != "path" && name != "filename" && name != "dir" && name != "dirname"
        && name != "file"
    {
        return None;
    }

    let (directory, file) = if crate::resolve::is_remote(url) {
        if url.starts_with("data:") {
            // A data URL has no path to take apart: its whole text is its name, and there is no
            // directory for a relative import to join against either.
            let text = if name == "url" { url } else { "" };
            return Some(quote(text));
        }
        // The URL one level up, which is what a relative import joins against and therefore the
        // only useful meaning of a directory for a remote module.
        let parsed = url::Url::parse(url).ok()?;
        let file = parsed.path().rsplit('/').next().unwrap_or_default().to_owned();
        let directory = parsed.join(".").ok()?.to_string();
        let directory = directory.trim_end_matches('/').to_owned();
        (directory, file)
    } else {
        let path = url.strip_prefix("file://")?;
        let directory = match path.rsplit_once('/') {
            Some((directory, _)) => directory,
            None => return None,
        };
        let file = path.rsplit('/').next().unwrap_or_default().to_owned();
        (directory.to_owned(), file)
    };

    let text = match name {
        "url" => url.to_owned(),
        "path" | "filename" => {
            if crate::resolve::is_remote(url) {
                // A remote module has no path; its URL is the closest honest answer, and saying
                // so beats handing back an empty string that looks like a real one.
                url.to_owned()
            } else {
                url.strip_prefix("file://")?.to_owned()
            }
        }
        "dir" | "dirname" => directory,
        "file" => file,
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
    // A module loaded from a URL is already identified by one.
    if crate::resolve::is_remote(path) {
        return path.to_owned();
    }
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
    fn a_remote_module_answers_from_its_url() {
        // Same property names, a different basis: there is no file, so dir is one URL level up --
        // which is exactly what a relative import from that module joins against.
        let url = "https://unpkg.com/a/dist/es/x.js";
        assert_eq!(value("url", url).unwrap(), quoted(url));
        assert_eq!(value("dir", url).unwrap(), quoted("https://unpkg.com/a/dist/es"));
        assert_eq!(value("dirname", url).unwrap(), quoted("https://unpkg.com/a/dist/es"));
        assert_eq!(value("file", url).unwrap(), quoted("x.js"));
    }

    #[test]
    fn a_data_url_has_no_directory_to_report() {
        let url = "data:text/javascript,export const x = 1";
        assert_eq!(value("url", url).unwrap(), quoted(url));
        assert_eq!(value("dir", url).unwrap(), "\"\"");
    }

    fn quoted(text: &str) -> String {
        serde_json::to_string(text).unwrap()
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
