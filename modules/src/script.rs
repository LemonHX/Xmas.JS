use std::path::{Path, PathBuf};
use std::str::FromStr;

use oxc::allocator::Allocator;
use oxc::ast::ast::Program;
use oxc::codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc::parser::{ParseOptions, Parser, ParserReturn};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{BabelOptions, TransformOptions, Transformer};
use rsquickjs::prelude::{Func, Rest};

use crate::utils::result::ResultExt;
pub fn allocator() -> Allocator {
    oxc::allocator::Allocator::default()
}
pub fn parse<'x>(
    source_type: &'x str,
    source: &'x str,
    allocator: &'x Allocator,
) -> Option<Program<'x>> {
    let source_type = match source_type {
        "mjs" => SourceType::mjs(),
        "cjs" => SourceType::cjs(),
        "jsx" => SourceType::jsx(),
        "ts" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        _ => SourceType::unambiguous(),
    };
    let ParserReturn {
        program,
        module_record,
        errors,
        panicked,
        ..
    } = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: true,
            ..ParseOptions::default()
        })
        .parse();
    if panicked {
        println!("Parser panicked");
        return None;
    } else {
        if !errors.is_empty() {
            println!("Parser Errors:");
            for error in errors {
                let error = error.with_source_code(source.to_string());
                println!("{error:?}");
            }
        }
        return Some(program);
    }
}

pub fn transform<'x>(
    source_path: &str,
    options: Option<BabelOptions>,
    minify: bool,
    allocator: &'x Allocator,
    mut ast: Program<'x>,
) -> rsquickjs::Result<String> {
    let scoping = SemanticBuilder::new().build(&ast).semantic.into_scoping();
    let transform_options = if let Some(babel) = options {
        TransformOptions::try_from(&babel).map_err(|e| {
            tracing::error!("Failed to convert Babel options: {:?}", e);
            rsquickjs::Error::new_from_js("TypeError", "Failed to convert Babel options")
        })?
    } else {
        let mut to = TransformOptions::enable_all();
        to.jsx.development = false;
        to.jsx.runtime = oxc::transformer::JsxRuntime::Classic;
        to.jsx.pragma = Some("_jsx.createElement".to_string());
        to.jsx.pragma_frag = Some("_jsx.Fragment".to_string());
        to
    };
    let trans = Transformer::new(&allocator, Path::new(source_path), &transform_options)
        .build_with_scoping(scoping, &mut ast);
    let codegen = Codegen::new().with_options(CodegenOptions {
        single_quote: false,
        minify,
        comments: if minify {
            CommentOptions::default()
        } else {
            CommentOptions::disabled()
        },
        source_map_path: Some(PathBuf::from_str(source_path).unwrap()),
        indent_char: oxc::codegen::IndentChar::Space,
        indent_width: 2,
        initial_indent: 0,
    });
    let output = codegen.build(&ast);
    return Ok(output.code);
}

pub fn script_transform<'js>(
    ctx: rsquickjs::Ctx<'js>,
    rest: Rest<rsquickjs::Value<'js>>,
) -> rsquickjs::Result<String> {
    let allocator = oxc::allocator::Allocator::default();

    // 0 th param should be the source code
    // 1 th optional param should be the source type: "js", "mjs", "cjs", "ts", "tsx", "jsx"
    // by default it is "tsx"
    // 2 th optional param should be babel options in json
    // 3 th optional param should be minify boolean
    let source = if let Some(v) = rest.get(0) {
        v.as_string().or_throw(&ctx)?.to_string().or_throw(&ctx)?
    } else {
        return Err(rsquickjs::Error::new_from_js(
            "TypeError",
            "First argument 'source' is required",
        ));
    };
    let source_type = if let Some(v) = rest.get(1) {
        v.as_string().or_throw(&ctx)?.to_string().or_throw(&ctx)?
    } else {
        "tsx".to_string()
    };

    let parsed = parse(&source_type, &source, &allocator);
    if let None = parsed {
        return Err(rsquickjs::Error::new_from_js(
            "Error",
            "Failed to parse source code",
        ));
    } else {
        let ast = parsed.unwrap();
        let babel_options = if let Some(v) = rest.get(2) {
            // let json_str = v.as_string().or_throw(ctx)?.to_string().or_throw(ctx)?;
            // let babel_opts: BabelOptions = serde_json::from_str(json_str).map_err(|e| {
            //     rsquickjs::Error::new_from_js(
            //         "TypeError",
            //         format!("Failed to parse babel options: {}", e),
            //     )
            // })?;
            // Some(babel_opts)
            tracing::warn!("Custom Babel options are not yet supported, using default options");
            None
        } else {
            None
        };
        let minify = if let Some(v) = rest.get(3) {
            v.as_bool().or_throw(&ctx)?
        } else {
            false
        };
        return Ok(transform(
            &format!("<transformed>.{}", source_type),
            babel_options,
            minify,
            &allocator,
            ast,
        )?);
    }
}

fn script_validate<'js>(
    ctx: rsquickjs::Ctx<'js>,
    rest: Rest<rsquickjs::Value<'js>>,
) -> rsquickjs::Result<bool> {
    let allocator = oxc::allocator::Allocator::default();

    // 0 th param should be the source code
    // 1 th optional param should be the source type: "js", "mjs", "cjs", "ts", "tsx", "jsx"
    // by default it is "tsx"
    let source = if let Some(v) = rest.get(0) {
        v.as_string().or_throw(&ctx)?.to_string().or_throw(&ctx)?
    } else {
        return Err(rsquickjs::Error::new_from_js(
            "TypeError",
            "First argument 'source' is required",
        ));
    };
    let source_type = if let Some(v) = rest.get(1) {
        v.as_string().or_throw(&ctx)?.to_string().or_throw(&ctx)?
    } else {
        "tsx".to_string()
    };

    let parsed = parse(&source_type, &source, &allocator);
    if let None = parsed {
        return Ok(false);
    }
    Ok(true)
}

fn script_eval<'js>(
    ctx: rsquickjs::Ctx<'js>,
    rest: Rest<rsquickjs::Value<'js>>,
) -> rsquickjs::Result<rsquickjs::Promise<'js>> {
    let transformed = script_transform(ctx.clone(), rest)?;
    ctx.eval_promise::<_>(transformed.as_bytes())
}

pub fn init(ctx: &rsquickjs::Ctx<'_>) -> rsquickjs::Result<()> {
    let globals = ctx.globals();
    // transform input script from jsx/ts/tsx to js
    globals.set("scriptTransform", Func::from(script_transform))?;
    // try to parse input script, return false if failed
    globals.set("scriptValidate", Func::from(script_validate))?;
    // validate and transform input script, evaluate if success, throw exception if failed
    globals.set("scriptEval", Func::from(script_eval))?;
    Ok(())
}

#[cfg(test)]
mod test {
    #[test]
    fn test_transformer_ts_to_js() {
        let source = r#"
        import { foo } from 'bar';

        interface Person {
            name: string;
            age: number;
        }

        const greet = (person: Person): string => {
            return `Hello, ${person.name}!`;
        };

        const user: Person = { name: "Jane Doe", age: 25 };
        console.log(greet(user));

        const x = <div>Hello, JSX!</div>;
        "#;
        let allocator = oxc::allocator::Allocator::default();
        let ast = super::parse("tsx", source, &allocator).unwrap();
        let r = super::transform("example.tsx", None, false, &allocator, ast).unwrap();
        println!("Transformed JS:\n{}", r);
    }
}
