//! CommonJS, by static scan.
//!
//! The engine runs modules only, and a host function cannot load a module while code is running, so a
//! `require()` that waits for its file is not on offer. What can be done is to read the file first: oxc
//! finds every `require("literal")`, and the file is rewritten into a module that imports those targets
//! up front and defines `require`, `module`, `exports`, `__filename` and `__dirname` around the
//! original body.
//!
//! What that means for callers, and what it costs:
//!
//! - `require("x")` needs a string literal. A computed specifier throws MODULE_NOT_FOUND saying so.
//! - Every static require runs before the file does, even one inside an `if` or a function.
//! - Targets that are CommonJS give `module.exports`; ESM files and the modules this runtime ships give
//!   their namespace (a shipped module gives its default export when it has one).
//! - A cycle hands back the exports object as it is at that moment, like node, except that each file's
//!   dependencies have all run before its own body starts.
//! - The body is module code, so it is strict: `with`, octal literals and `arguments.callee` do not parse.
//!
//! A file is wrapped when it has no `import` or `export` and uses `require`, `module`, `exports`,
//! `__filename` or `__dirname` without declaring it, or is called `.cjs`. A module that calls a free
//! `require` gets just that function (as in LLRT), with no shell and no `module` or `exports`.

use oxc::allocator::Allocator;
use oxc::ast::ast::{Argument, CallExpression, Expression, IdentifierReference};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;

const NAMES: [&str; 5] = ["require", "module", "exports", "__filename", "__dirname"];

#[derive(Default)]
struct Scan {
    requires: Vec<String>,
    free_name: bool,
}

impl<'a> Visit<'a> for Scan {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if NAMES.contains(&it.name.as_str()) {
            self.free_name = true;
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && callee.name == "require"
            && call.arguments.len() == 1
            && let Argument::StringLiteral(text) = &call.arguments[0]
        {
            let spec = text.value.to_string();
            if !self.requires.contains(&spec) {
                self.requires.push(spec);
            }
        }
        walk::walk_call_expression(self, call);
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The shell around `code` when it is a CommonJS file, or just a `require` when it is a module that
/// calls one; nothing when it needs neither. `keep` says which specifiers are names the engine already
/// knows (the shipped modules), which are imported as written.
pub fn wrap(path: &str, code: &str, keep: &dyn Fn(&str) -> bool) -> Option<String> {
    let cjs_name = path.ends_with(".cjs");
    if !cjs_name && !NAMES.iter().any(|name| code.contains(name)) {
        return None;
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::unambiguous())
        .with_options(ParseOptions {
            parse_regular_expression: false,
            allow_return_outside_function: true,
            ..ParseOptions::default()
        })
        .parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let is_module = parsed.module_record.has_module_syntax;
    // Which of the five names the file uses without declaring them itself.
    let unresolved: Vec<&str> = {
        let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
        let scoping = semantic.scoping();
        NAMES
            .into_iter()
            .filter(|name| {
                scoping
                    .root_unresolved_references()
                    .keys()
                    .any(|key| key.to_string() == *name)
            })
            .collect()
    };
    let wants_require = unresolved.contains(&"require");
    let is_cjs = !is_module && (cjs_name || !unresolved.is_empty());
    if !is_cjs && !(is_module && wants_require) {
        return None;
    }
    let mut scan = Scan::default();
    scan.visit_program(&parsed.program);

    let mut shell = String::new();
    let mut table = Vec::new();
    for (index, spec) in scan.requires.iter().enumerate() {
        // A file that resolves is imported by its path, so the key is the same whoever asks. A name
        // that does not resolve is left to the loader, which may install the package.
        let key = if keep(spec) {
            spec.clone()
        } else {
            crate::resolve::resolve_require(path, spec).unwrap_or_else(|_| spec.clone())
        };
        shell.push_str(&format!("import * as __d{index} from {};", quote(&key)));
        table.push(format!("[{},[__d{index},{}]]", quote(spec), quote(&key)));
    }
    let quoted = quote(path);
    shell.push_str(concat!(
        "const __r=(globalThis[Symbol.for('xmas.cjs')]??=new Map());",
        "const __g=k=>{let m=__r.get(k);if(!m){m={exports:{},id:k};__r.set(k,m)}return m};",
    ));
    if is_cjs {
        let directory = path.rsplit_once('/').map_or(".", |(directory, _)| directory);
        shell.push_str(&format!(
            "const module=__g({quoted});module.filename={quoted};module.cjs=true;\
             const exports=module.exports;const __filename={quoted};const __dirname={};",
            quote(directory),
        ));
    }
    shell.push_str(&format!("const __deps=new Map([{}]);", table.join(",")));
    shell.push_str(concat!(
        "const require=s=>{const d=__deps.get(s);",
        "if(!d)throw Object.assign(new Error(`Cannot find module '${s}': require is resolved by static scan, so the specifier must be a string literal`),{code:'MODULE_NOT_FOUND'});",
        "const[ns,k]=d;const m=__r.get(k);if(m&&m.cjs)return m.exports;",
        "if(k.endsWith('.json'))return ns.default;",
        "if(k[0]!=='/'&&k[1]!==':')return ns.default??ns;",
        "try{void ns.default;return ns}catch(e){if(e instanceof ReferenceError)return __g(k).exports;throw e}};",
    ));
    if !is_cjs {
        shell.push_str(code);
        return Some(shell);
    }
    shell.push_str(";(function(exports,require,module,__filename,__dirname){");
    shell.push_str(code);
    shell.push_str(
        "\n}).call(module.exports,exports,require,module,__filename,__dirname);module.loaded=true;export default module.exports;\n",
    );
    Some(shell)
}
