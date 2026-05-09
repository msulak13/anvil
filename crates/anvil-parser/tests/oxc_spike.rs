//! ADR-0001 spike: confirm `oxc_parser` can parse the three v0.1 fixture
//! shapes — `@Module` + `@Provides`, `@Inject` ctor, and `@Singleton` —
//! without errors and exposes the decorator AST in a usable form.
//!
//! If any of these tests fail, ADR 0001 needs to be superseded with a switch
//! to SWC before M2.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

const SIMPLE_PROVIDES: &str = r#"
import { Module, Provides, Component } from "@msulak/anvil";

export class Pump {
  constructor() {}
}

@Module
export class CoffeeModule {
  @Provides static providePump(): Pump { return new Pump(); }
}

@Component({ modules: [CoffeeModule] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
}
"#;

const INJECT_CTOR: &str = r#"
import { Inject, Component } from "@msulak/anvil";

@Inject
export class Heater {
  constructor() {}
}

@Inject
export class Pump {
  constructor(private heater: Heater) {}
}

@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
}
"#;

const SINGLETON_SCOPE: &str = r#"
import { Inject, Singleton, Component } from "@msulak/anvil";

@Inject
@Singleton
export class Heater {
  constructor() {}
}

@Inject
export class Pump {
  constructor(private heater: Heater) {}
}

@Singleton
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
  abstract heater(): Heater;
}
"#;

fn parse(source: &str) -> usize {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, source, source_type).parse();
    assert!(ret.errors.is_empty(), "oxc parse errors: {:#?}", ret.errors);
    let mut decorated_classes = 0usize;
    for stmt in &ret.program.body {
        // Decorated classes appear as `Statement::ExportNamedDeclaration` (export class…)
        // or `Statement::ClassDeclaration` (bare class…). We accept both.
        let class = match stmt {
            Statement::ExportNamedDeclaration(decl) => match &decl.declaration {
                Some(Declaration::ClassDeclaration(c)) => Some(c),
                _ => None,
            },
            Statement::ClassDeclaration(c) => Some(c),
            _ => None,
        };
        if let Some(class) = class {
            if !class.decorators.is_empty() {
                decorated_classes += 1;
            }
            // Confirm method-level decorators are reachable too.
            for member in &class.body.body {
                if let oxc_ast::ast::ClassElement::MethodDefinition(m) = member {
                    if !m.decorators.is_empty() {
                        decorated_classes += 1;
                    }
                }
            }
        }
    }
    decorated_classes
}

#[test]
fn parses_simple_provides_with_decorators() {
    // 1 class decorator on CoffeeModule + 1 method decorator on providePump
    // + 1 class decorator on CoffeeShop = 3.
    assert_eq!(parse(SIMPLE_PROVIDES), 3);
}

#[test]
fn parses_inject_ctor() {
    // TC39 Stage-3 decorators don't decorate constructors, so @Inject lives
    // on the class. Heater + Pump get @Inject, CoffeeShop gets @Component =
    // 3 class-level decorators total.
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, INJECT_CTOR, source_type).parse();
    assert!(ret.errors.is_empty(), "oxc parse errors: {:#?}", ret.errors);

    let mut total_class_decorators = 0usize;
    for stmt in &ret.program.body {
        if let Statement::ExportNamedDeclaration(decl) = stmt {
            if let Some(Declaration::ClassDeclaration(c)) = &decl.declaration {
                total_class_decorators += c.decorators.len();
            }
        }
    }
    assert_eq!(total_class_decorators, 3);
}

#[test]
fn parses_singleton_scope() {
    // (@Inject + @Singleton) on Heater = 2; @Inject on Pump = 1;
    // (@Singleton + @Component) on CoffeeShop = 2. Total = 5 class-level
    // decorators. Confirms the parser handles stacked decorators.
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, SINGLETON_SCOPE, source_type).parse();
    assert!(ret.errors.is_empty(), "oxc parse errors: {:#?}", ret.errors);

    let mut total_class_decorators = 0usize;
    for stmt in &ret.program.body {
        if let Statement::ExportNamedDeclaration(decl) = stmt {
            if let Some(Declaration::ClassDeclaration(c)) = &decl.declaration {
                total_class_decorators += c.decorators.len();
            }
        }
    }
    assert_eq!(total_class_decorators, 5);
}
