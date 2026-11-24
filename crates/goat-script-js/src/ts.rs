//! TypeScript → JavaScript (type stripping + modern syntax kept) with oxc.

use std::path::Path;

/// Compiles a `.ts` source to an ES module (imports / exports kept).
pub fn compile(key: &str, source: &str) -> Result<String, String> {
    use oxc_allocator::Allocator;
    use oxc_codegen::Codegen;
    use oxc_parser::Parser;
    use oxc_semantic::SemanticBuilder;
    use oxc_span::SourceType;
    use oxc_transformer::{TransformOptions, Transformer};

    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "));
    }
    let mut program = parsed.program;
    let semantic = SemanticBuilder::new().build(&program);
    if !semantic.diagnostics.is_empty() {
        return Err(semantic.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "));
    }
    let scoping = semantic.semantic.into_scoping();
    let options = TransformOptions::default();
    let ret = Transformer::new(&allocator, Path::new(key), &options).build_with_scoping(scoping, &mut program);
    if !ret.diagnostics.is_empty() {
        return Err(ret.diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "));
    }
    Ok(Codegen::new().build(&program).code)
}
