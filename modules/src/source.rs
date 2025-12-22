use std::path::{Path, PathBuf};
use std::str::FromStr;

use oxc::allocator::Allocator;
use oxc::ast::ast::Program;
use oxc::codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc::parser::{ParseOptions, Parser, ParserReturn};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};

fn parse<'x>(source_type: &'x str, source: &'x str, allocator: &'x Allocator) -> Option<Program<'x>> {
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

fn transform<'x>(source_path: &str, minify: bool, allocator: &'x Allocator, mut ast: Program<'x>) {
    let scoping = SemanticBuilder::new().build(&ast).semantic.into_scoping();
    let transform_options = TransformOptions::enable_all();
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
    let _output = codegen.build(&ast);
    println!("{}", _output.code);
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
        super::transform("example.tsx", false, &allocator, ast);
    }
}