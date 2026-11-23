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
//! The properties are the ones Bun documents. The path group is answered from the file's own path,
//! so a module read from disk says where it is on disk and a module defined in memory says so.
//! `main` is a comparison against the entry, and `env` is the environment snapshot the realm was
//! built with -- both of which the module table knows and a path does not.

use std::collections::BTreeMap;

use std::path::Path;

use oxc::allocator::Allocator;
use oxc::ast::ast::Expression;
use oxc::ast_visit::{Visit, walk};
use oxc::parser::{Parser, ParseOptions};
use oxc::span::{GetSpan, SourceType};

/// What a module's `import.meta` is answered from, besides its own path.
///
/// The path group needs nothing more. `main` needs to know which module the process started at,
/// and `env` needs the snapshot the realm was built with -- the compiler is a pure function of
/// (path, source) otherwise, and this is the one thing it cannot work out for itself.
#[derive(Clone, Default)]
pub struct Context {
    /// The entry module, as the loader names it. A module is `main` when its name is this.
    pub main: Option<String>,
    /// Environment variables, when the realm has them.
    pub env: Option<BTreeMap<String, String>>,
}

/// One replacement: the bytes to overwrite, and what to put there.
struct Splice {
    start: usize,
    end: usize,
    text: String,
}

struct Scan<'a> {
    /// The file this code came from, as a `file://` URL.
    url: &'a str,
    /// The name the loader knows this module by, for `main` to compare against.
    name: &'a str,
    context: &'a Context,
    /// The source being rewritten, so a call's own arguments can be copied through as written.
    source: &'a str,
    found: Vec<Splice>,
    /// A use this layer cannot answer, kept so the caller can say which one.
    unknown: Option<String>,
}

impl<'a> Visit<'a> for Scan<'a> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        // `import.meta.resolve(specifier, parent?)` is a call, not a value: the specifier may be a
        // variable, so it cannot become a literal. What can be known at compile time is which
        // module is asking, so that is what gets baked in -- the specifier is passed through.
        if let Expression::CallExpression(call) = expression
            && let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::MetaProperty(meta) = &member.object
            && meta.meta.name == "import"
            && meta.property.name == "meta"
            && member.property.name == "resolve"
        {
            let mut text = format!("globalThis.__xmas_resolve({}", quote(self.url));
            for argument in &call.arguments {
                text.push(',');
                text.push_str(&self.source[argument.span().start as usize..argument.span().end as usize]);
            }
            text.push(')');
            self.found.push(Splice {
                start: meta.span.start as usize,
                end: call.span.end as usize,
                text,
            });
            return;
        }
        if let Expression::StaticMemberExpression(member) = expression
            && let Expression::MetaProperty(meta) = &member.object
            && meta.meta.name == "import"
            && meta.property.name == "meta"
        {
            let name = member.property.name.as_str();
            match value_in(name, self.url, self.name, self.context) {
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

/// The property, taking into account what the path group cannot know.
fn value_in(name: &str, url: &str, module: &str, context: &Context) -> Option<String> {
    match name {
        // Whether this module is the one the process started at. A module with no entry recorded
        // is not the main module: a realm built to run a test is nobody's process entry.
        "main" => Some(
            match &context.main {
                Some(main) => main == module,
                None => false,
            }
            .to_string(),
        ),
        // The environment as an object literal, so `import.meta.env.X` reads like a property of
        // one. A property that is not there is `undefined`, which is what a JS object does on its
        // own -- there is no Proxy here and none is needed.
        "env" => Some(match &context.env {
            Some(vars) => env_object(vars),
            None => "undefined".to_owned(),
        }),
        // Bun also answers MODE / DEV / PROD on the same object. They are three names over one
        // value, and reading NODE_ENV once is cheaper than three properties the program may never
        // touch -- so they are hung on the object, not looked up separately here.
        _ => value(name, url),
    }
}

/// An object literal holding the whole environment.
///
/// Every key is quoted, because an environment variable may be named anything: `FOO-BAR` is a
/// legal name in a shell and not a legal identifier in JavaScript. A shallow object with no
/// prototype, so `env.toString` is undefined rather than a function nobody put there.
///
/// Bun's three release flags are derived from NODE_ENV here rather than being separate reads:
/// PROD when NODE_ENV is exactly "production", DEV when it is anything else, MODE its value.
fn env_object(vars: &BTreeMap<String, String>) -> String {
    let mode = vars.get("NODE_ENV").map(String::as_str).unwrap_or("development");
    let production = mode == "production";
    let mut out = String::from("(() => { const e = Object.assign(Object.create(null), {");
    let mut first = true;
    for (key, value) in vars {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&quote(key));
        out.push(':');
        out.push_str(&quote(value));
    }
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str("PROD");
    out.push('"');
    out.push(':');
    out.push_str(if production { "true" } else { "false" });
    out.push_str(",\"DEV\":");
    out.push_str(if production { "false" } else { "true" });
    out.push_str(",\"MODE\":");
    out.push_str(&quote(mode));
    out.push_str("});");
    // Frozen, so a module cannot make another module's environment disagree with it by writing to
    // the object it happened to see first.
    out.push_str(" return Object.freeze(e); })()");
    out
}

fn name_of(name: &str) -> String {
    format!("import.meta.{name} is not implemented yet")
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
pub fn rewrite(source: &str, path: &str, context: &Context) -> Result<Option<String>, String> {
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
        name: path,
        context,
        source,
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
            rewrite("export const u = import.meta.url;", "/a/b/c.js", &Context::default()).unwrap().as_deref(),
            Some("export const u = \"file:///a/b/c.js\";")
        );
    }

    #[test]
    fn the_path_group_is_answered() {
        let code = "const a = import.meta.dir, b = import.meta.file, c = import.meta.path;";
        assert_eq!(
            rewrite(code, "/a/b/c.js", &Context::default()).unwrap().as_deref(),
            Some("const a = \"/a/b\", b = \"c.js\", c = \"/a/b/c.js\";")
        );
    }

    #[test]
    fn several_uses_keep_their_order() {
        let code = "const x = [import.meta.url, import.meta.file];";
        assert_eq!(
            rewrite(code, "/a/c.js", &Context::default()).unwrap().as_deref(),
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
        assert!(rewrite("export const x = 1;", "/a/b.js", &Context::default()).unwrap().is_none());
    }

    #[test]
    fn the_main_flag_is_a_comparison_against_the_entry() {
        let context = Context {
            main: Some("/a/b.js".to_owned()),
            ..Context::default()
        };
        assert_eq!(
            rewrite("const m = import.meta.main;", "/a/b.js", &context).unwrap().as_deref(),
            Some("const m = true;")
        );
        // A module the program did not start at is not main, however close to the entry it is.
        assert_eq!(
            rewrite("const m = import.meta.main;", "/a/c.js", &context).unwrap().as_deref(),
            Some("const m = false;")
        );
        // A realm with no entry recorded -- a test, an embedded realm -- is nobody's main.
        assert_eq!(
            rewrite("const m = import.meta.main;", "/a/b.js", &Context::default())
                .unwrap()
                .as_deref(),
            Some("const m = false;")
        );
    }

    #[test]
    fn the_environment_is_an_object_with_buns_release_flags() {
        let context = Context {
            env: Some(
                [
                    ("NODE_ENV".to_owned(), "production".to_owned()),
                    ("A-B".to_owned(), "x\"y".to_owned()),
                ]
                .into_iter()
                .collect(),
            ),
            ..Context::default()
        };
        let out = rewrite("export const e = import.meta.env;", "/a/b.js", &context)
            .unwrap()
            .unwrap();
        // Every key is quoted: an environment name is anything a shell allows, and A-B is not an
        // identifier in JavaScript.
        assert!(out.contains("\"A-B\":\"x\\\"y\""), "{out}");
        assert!(out.contains("\"PROD\":true"), "{out}");
        assert!(out.contains("\"DEV\":false"), "{out}");
        assert!(out.contains("\"MODE\":\"production\""), "{out}");
    }

    #[test]
    fn a_realm_with_no_environment_says_undefined() {
        // Not an empty object: a runtime embedded in something without an environment has no
        // answer, and an object would be a lie about having one.
        assert_eq!(
            rewrite("export const e = import.meta.env;", "/a/b.js", &Context::default())
                .unwrap()
                .as_deref(),
            Some("export const e = undefined;")
        );
    }

    #[test]
    fn a_property_this_layer_cannot_answer_is_named() {
        let error = rewrite("const m = import.meta.resolve;", "/a/b.js", &Context::default())
            .unwrap_err();
        assert!(error.contains("import.meta.resolve"), "{error}");
    }
}
