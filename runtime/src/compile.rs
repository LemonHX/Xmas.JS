//! Compilation layer: turn ts / tsx into something the engine can directly consume; plain js does not go through oxc.
//!
//! For configuration, use only TransformOptions::default(), and explicitly touch only the jsx part:
//! Use the classic runtime, set the pragma to h / Fragment, do not import react, and do not enable react-refresh.
//! This way, the output entering the engine does not depend on any module resolution.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};

/// There should not be even one compilation on the request path. This counter is used to prove that.
pub static COMPILES: AtomicUsize = AtomicUsize::new(0);

pub fn compile_count() -> usize {
    COMPILES.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Plain js: pass through directly; do not go through oxc
    Js,
    Ts,
    Tsx,
    Jsx,
}

pub fn kind_of(path: &str) -> Kind {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let ext = path.rsplit('.').next().unwrap_or_default();
    match ext {
        "ts" | "mts" | "cts" => Kind::Ts,
        "tsx" => Kind::Tsx,
        "jsx" => Kind::Jsx,
        _ => Kind::Js,
    }
}

fn source_type(kind: Kind) -> SourceType {
    match kind {
        Kind::Js => SourceType::unambiguous(),
        Kind::Ts => SourceType::ts(),
        Kind::Tsx => SourceType::tsx(),
        Kind::Jsx => SourceType::jsx(),
    }
}

fn transform_options() -> TransformOptions {
    TransformOptions {
        jsx: JsxOptions {
            runtime: JsxRuntime::Classic,
            development: false,
            pragma: Some("h".to_owned()),
            pragma_frag: Some("Fragment".to_owned()),
            ..JsxOptions::default()
        },
        ..TransformOptions::default()
    }
}

/// Compile a piece of source code. Kind::Js is returned directly as-is, without even creating an allocator.
pub fn compile(path: &str, source: &str) -> Result<String, String> {
    let kind = kind_of(path);
    if kind == Kind::Js {
        return Ok(source.to_owned());
    }
    COMPILES.fetch_add(1, Ordering::Relaxed);

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type(kind))
        .with_options(ParseOptions {
            // Parse regex literals on demand; do not run through every /.../ during the parse phase.
            parse_regular_expression: false,
            ..ParseOptions::default()
        })
        .parse();
    if parsed.panicked {
        return Err(format!("{path}: parser panicked"));
    }
    if let Some(error) = parsed.errors.first() {
        return Err(format!("{path}: {error:?}"));
    }
    let mut program = parsed.program;

    let scoping = SemanticBuilder::new()
        .build(&program)
        .semantic
        .into_scoping();
    let options = transform_options();
    let transformed = Transformer::new(&allocator, Path::new(path), &options)
        .build_with_scoping(scoping, &mut program);
    if let Some(error) = transformed.errors.first() {
        return Err(format!("{path}: {error:?}"));
    }

    let codegen = Codegen::new().with_options(CodegenOptions {
        single_quote: false,
        minify: false,
        comments: CommentOptions::disabled(),
        source_map_path: None,
        indent_char: oxc::codegen::IndentChar::Space,
        indent_width: 2,
        initial_indent: 0,
    });
    Ok(codegen.build(&program).code)
}
